//! Durable overflow for the execution queue.
//!
//! When enabled (via `[executor.queue_overflow]`), messages that would otherwise be
//! dropped are serialized, compressed, and written to a dedicated storage namespace so
//! they survive a queue-full burst or an ungraceful shutdown, and are replayed on a
//! later boot instead of being lost.
//!
//! # Durability model
//!
//! Messages live in two namespaces:
//! - **ready** (`queue_overflow`): messages waiting to be reinjected
//! - **inflight** (`queue_overflow_inflight`): messages claimed by a reloader but not yet
//!   successfully enqueued (or returned to ready)
//!
//! Claim protocol (at-least-once under multi-replica):
//! 1. Atomically claim a ready row via `delete` (only one replica gets the value).
//! 2. **Immediately** park the raw bytes in the inflight namespace (lease). This closes the
//!    crash window for the subsequent decompress / enqueue work: if we die after this
//!    point, a later reloader reclaims the expired lease and puts the message back to ready.
//! 3. Enqueue into the executor. On success, delete the inflight row (ack). On queue full
//!    or disconnect, move the original bytes back to ready under the same key and ack
//!    inflight.
//!
//! Residual crash window: between `delete(ready)` and `insert(inflight)` (one storage
//! round-trip). Closing that fully requires compare-and-swap / transactions in the storage
//! trait. Everything after the inflight park is recoverable.
//!
//! At-least-once: a crash after a successful `try_send` but before inflight ack can cause
//! a duplicate reinject once the lease expires. Rules must tolerate reprocessing.
//!
//! Multi-replica: delete-returns-value ensures a given ready row is claimed by at most one
//! reloader at a time. Cross-pod recovery requires a *shared* backend (e.g. DynamoDB).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::TrySendError;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use plaid_stl::messages::{LogSource, LogbacksAllowed};
use serde::{Deserialize, Serialize};

use crate::config::QueueOverflowConfig;
use crate::executor::{Executor, Message};
use crate::storage::Storage;

/// Storage namespace holding ready (not yet claimed) overflow messages.
pub const QUEUE_OVERFLOW_NS: &str = "queue_overflow";

/// Storage namespace for claimed-but-not-acked messages. Leased rows here are reclaimed
/// back into the ready namespace after `claim_lease_secs`.
pub const QUEUE_OVERFLOW_INFLIGHT_NS: &str = "queue_overflow_inflight";

/// DynamoDB caps a single item at 400 KB (key + all attributes). We refuse to persist a
/// compressed message larger than this, leaving headroom for the key and attribute
/// overhead, and log a distinct error instead of letting the backend reject the write.
const MAX_ITEM_BYTES: usize = 380 * 1024;

/// Wire format version for stored message envelopes (base64 payloads).
const STORED_MESSAGE_VERSION: u8 = 1;

/// Even forced persists (shutdown) refuse to grow the store beyond this multiple of
/// `max_persisted`, so a crash loop cannot fill the backend without bound.
const FORCED_CAP_MULTIPLIER: u64 = 2;

/// Log a warning when the ready namespace exceeds this many keys (list cost / memory).
const LIST_SIZE_WARN_THRESHOLD: usize = 10_000;

/// The result of trying to persist a single message to the overflow store.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PersistOutcome {
    /// The message was written to durable storage.
    Persisted,
    /// The overflow store already holds `max_persisted` messages; caller should drop.
    CapExceeded,
    /// The message is a GET-mode request (carries a response channel) and cannot be
    /// meaningfully replayed, so it is not persisted.
    NotReplayable,
    /// The compressed message exceeds the per-item storage limit.
    TooLarge,
    /// Serialization or the storage write failed.
    Failed,
}

impl PersistOutcome {
    /// Human-readable label for logs / metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Persisted => "persisted",
            Self::CapExceeded => "cap_exceeded",
            Self::NotReplayable => "not_replayable",
            Self::TooLarge => "too_large",
            Self::Failed => "failed",
        }
    }

    /// Log a non-success outcome at an appropriate level. No-op for `Persisted`.
    pub fn log_if_not_persisted(self, context: &str, message_id: &str, source: &LogSource) {
        match self {
            Self::Persisted => {}
            Self::CapExceeded => {
                error!(
                    "Overflow {context}: cap exceeded, dropping message {message_id} from {source}"
                );
            }
            Self::NotReplayable => {
                debug!(
                    "Overflow {context}: message {message_id} from {source} is not replayable (GET-mode); not persisted"
                );
            }
            Self::TooLarge => {
                // persist path already logs details; keep a one-liner here for callers
                // that only see the outcome.
                error!(
                    "Overflow {context}: message {message_id} from {source} exceeds item size limit"
                );
            }
            Self::Failed => {
                error!(
                    "Overflow {context}: failed to persist message {message_id} from {source}"
                );
            }
        }
    }
}

/// Compact on-disk envelope: binary fields are base64 so gzip sees compressible text and
/// we avoid JSON's array-of-bytes blow-up (which caused many false `TooLarge` drops).
#[derive(Serialize, Deserialize)]
struct StoredMessageV1 {
    v: u8,
    id: String,
    type_: String,
    data_b64: String,
    headers: HashMap<String, String>,
    query_params: HashMap<String, String>,
    source: LogSource,
    logbacks_allowed: LogbacksAllowed,
}

impl StoredMessageV1 {
    fn from_message(message: &Message) -> Self {
        Self {
            v: STORED_MESSAGE_VERSION,
            id: message.id.clone(),
            type_: message.type_.clone(),
            data_b64: base64::encode(&message.data),
            headers: message
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), base64::encode(v)))
                .collect(),
            query_params: message
                .query_params
                .iter()
                .map(|(k, v)| (k.clone(), base64::encode(v)))
                .collect(),
            source: message.source.clone(),
            logbacks_allowed: message.logbacks_allowed.clone(),
        }
    }

    fn into_message(self) -> Result<Message, String> {
        let data = base64::decode(&self.data_b64).map_err(|e| format!("data base64: {e}"))?;
        let mut headers = HashMap::new();
        for (k, v) in self.headers {
            headers.insert(
                k,
                base64::decode(&v).map_err(|e| format!("header base64: {e}"))?,
            );
        }
        let mut query_params = HashMap::new();
        for (k, v) in self.query_params {
            query_params.insert(
                k,
                base64::decode(&v).map_err(|e| format!("query base64: {e}"))?,
            );
        }
        Ok(Message {
            id: self.id,
            type_: self.type_,
            data,
            headers,
            query_params,
            source: self.source,
            logbacks_allowed: self.logbacks_allowed,
            response_sender: None,
            module: None,
        })
    }
}

/// Durable overflow store for execution-queue messages.
pub struct OverflowStore {
    storage: Arc<Storage>,
    config: QueueOverflowConfig,
    /// Approximate number of messages currently held in ready + inflight by *this*
    /// process's accounting. Seeded at startup and adjusted as we persist / ack.
    ///
    /// Per-pod (not a global hard cap): with N replicas the true occupancy can approach
    /// roughly `N * max_persisted`. It is a blast-radius bound, not exact distributed
    /// accounting. Uses saturating arithmetic so concurrent claims can never wrap the
    /// counter into a permanent self-DoS.
    count: AtomicU64,
}

impl OverflowStore {
    /// Build a store and seed the approximate count from ready + inflight namespaces.
    pub async fn new(storage: Arc<Storage>, config: QueueOverflowConfig) -> Self {
        let ready = match storage.list_keys(QUEUE_OVERFLOW_NS, None).await {
            Ok(keys) => keys.len() as u64,
            Err(e) => {
                error!("Could not seed overflow ready count from storage: {e}. Starting that half from 0.");
                0
            }
        };
        let inflight = match storage.list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None).await {
            Ok(keys) => keys.len() as u64,
            Err(e) => {
                error!("Could not seed overflow inflight count from storage: {e}. Starting that half from 0.");
                0
            }
        };
        let count = ready.saturating_add(inflight);
        info!(
            "Overflow store initialized with {count} persisted message(s) ({ready} ready, {inflight} inflight); max_persisted={}",
            config.max_persisted
        );
        Self {
            storage,
            config,
            count: AtomicU64::new(count),
        }
    }

    /// Current approximate persisted count (ready + inflight) for this process.
    pub fn approximate_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Persist a single message, subject to the configured cap. Never blocks on the
    /// executor; only touches storage. Used on the hot path (queue-full spill).
    pub async fn persist(&self, message: &Message) -> PersistOutcome {
        self.persist_with_key(message, false, None).await
    }

    /// Persist a single message, bypassing the soft cap up to a hard multiple of
    /// `max_persisted`. Used at shutdown / repersist, where dropping would be permanent
    /// data loss — but still refuse unbounded growth across crash loops.
    pub async fn persist_forced(&self, message: &Message) -> PersistOutcome {
        self.persist_with_key(message, true, None).await
    }

    /// Persist `message`. If `existing_key` is given (a message being put back after a
    /// failed reinject), reuse it so the original creation time, and therefore the age
    /// used by the reaper, is preserved. Otherwise mint a fresh timestamped key.
    ///
    /// When `existing_key` is set we do **not** bump the count: the message was already
    /// accounted for (claimed into inflight without decrementing).
    async fn persist_with_key(
        &self,
        message: &Message,
        bypass_soft_cap: bool,
        existing_key: Option<String>,
    ) -> PersistOutcome {
        // GET-mode messages block an HTTP client on a response channel that cannot be
        // serialized or reconstructed on reload, so replaying them is pointless.
        if message.response_sender.is_some() {
            return PersistOutcome::NotReplayable;
        }

        let count = self.count.load(Ordering::Relaxed);
        if bypass_soft_cap {
            let hard = self
                .config
                .max_persisted
                .saturating_mul(FORCED_CAP_MULTIPLIER);
            if count >= hard {
                error!(
                    "Overflow hard cap ({hard}) reached during forced persist; dropping message {} from {}",
                    message.id, message.source
                );
                return PersistOutcome::CapExceeded;
            }
        } else if count >= self.config.max_persisted {
            return PersistOutcome::CapExceeded;
        }

        let encoded = match encode_message(message) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!(
                    "Failed to encode overflow message {} from {}: {e}",
                    message.id, message.source
                );
                return PersistOutcome::Failed;
            }
        };

        if encoded.len() > MAX_ITEM_BYTES {
            error!(
                "Overflow message {} from {} is {} bytes compressed, over the {MAX_ITEM_BYTES} byte item limit; dropping to avoid a storage rejection",
                message.id,
                message.source,
                encoded.len()
            );
            return PersistOutcome::TooLarge;
        }

        let is_restore = existing_key.is_some();
        let key = existing_key.unwrap_or_else(|| overflow_key(now_millis(), &message.id));
        match self
            .storage
            .insert(QUEUE_OVERFLOW_NS.to_string(), key, encoded)
            .await
        {
            Ok(previous) => {
                // Only count a net-new row. Restores and overwrites of an existing key
                // must not inflate the approximate counter.
                if !is_restore && previous.is_none() {
                    self.count.fetch_add(1, Ordering::Relaxed);
                }
                PersistOutcome::Persisted
            }
            Err(e) => {
                error!(
                    "Storage could not persist overflow message {} from {}: {e}",
                    message.id, message.source
                );
                PersistOutcome::Failed
            }
        }
    }

    /// Drop ready messages older than `max_message_age_secs` without reinjecting them.
    /// Safe to run while the executor queue is full (unlike claim/reinject).
    ///
    /// Returns the number of messages reaped.
    pub async fn reap_expired(&self) -> usize {
        let keys = match self.storage.list_keys(QUEUE_OVERFLOW_NS, None).await {
            Ok(keys) => keys,
            Err(e) => {
                error!("Could not list overflow messages to reap: {e}");
                return 0;
            }
        };

        if keys.is_empty() {
            return 0;
        }

        let now = now_secs();
        let mut reaped = 0usize;

        for key in keys {
            let Some(created) = millis_from_key(&key) else {
                // Malformed keys never age out via the normal path; delete them so they
                // cannot pin the namespace forever.
                warn!("Reaping overflow message with unparseable key {key}");
                match self.storage.delete(QUEUE_OVERFLOW_NS, &key).await {
                    Ok(Some(_)) => {
                        self.saturating_dec();
                        reaped += 1;
                    }
                    Ok(None) => {}
                    Err(e) => error!("Could not reap malformed overflow key {key}: {e}"),
                }
                continue;
            };
            let age_secs = now.saturating_sub(created / 1000);
            if age_secs <= self.config.max_message_age_secs {
                continue;
            }
            match self.storage.delete(QUEUE_OVERFLOW_NS, &key).await {
                Ok(Some(_)) => {
                    warn!("Reaped overflow message {key}: age {age_secs}s exceeds limit");
                    self.saturating_dec();
                    reaped += 1;
                }
                Ok(None) => {}
                Err(e) => error!("Could not reap overflow message {key}: {e}"),
            }
        }

        if reaped > 0 {
            info!("Reaped {reaped} expired overflow message(s)");
        }
        reaped
    }

    /// Move expired inflight leases back to the ready namespace so they can be claimed
    /// again after a crash mid-reload. Returns how many rows were reclaimed.
    pub async fn reclaim_stale_inflight(&self) -> usize {
        let keys = match self
            .storage
            .list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                error!("Could not list inflight overflow messages: {e}");
                return 0;
            }
        };

        if keys.is_empty() {
            return 0;
        }

        let now = now_secs();
        let lease = self.config.claim_lease_secs;
        let mut reclaimed = 0usize;

        for key in keys {
            let value = match self.storage.get(QUEUE_OVERFLOW_INFLIGHT_NS, &key).await {
                Ok(Some(v)) => v,
                Ok(None) => continue,
                Err(e) => {
                    error!("Could not read inflight overflow {key}: {e}");
                    continue;
                }
            };
            let Some((claimed_at, body)) = unwrap_inflight(&value) else {
                warn!("Dropping corrupt inflight overflow record {key}");
                let _ = self.storage.delete(QUEUE_OVERFLOW_INFLIGHT_NS, &key).await;
                self.saturating_dec();
                continue;
            };
            if now.saturating_sub(claimed_at) < lease {
                continue; // still within lease; another reloader may be working it
            }

            // Claim the inflight row so only one reclaimer restores it.
            match self.storage.delete(QUEUE_OVERFLOW_INFLIGHT_NS, &key).await {
                Ok(Some(_)) => {}
                Ok(None) => continue, // lost the race
                Err(e) => {
                    error!("Could not claim stale inflight overflow {key}: {e}");
                    continue;
                }
            }

            // Put original ready bytes back under the original key (preserves age).
            match self
                .storage
                .insert(QUEUE_OVERFLOW_NS.to_string(), key.clone(), body)
                .await
            {
                Ok(_) => {
                    info!("Reclaimed stale inflight overflow message {key} back to ready");
                    reclaimed += 1;
                }
                Err(e) => {
                    error!(
                        "Failed to restore reclaimed inflight overflow {key} to ready: {e}; message may be lost"
                    );
                    self.saturating_dec();
                }
            }
        }

        reclaimed
    }

    /// Claim and reinject up to `reload_batch_size` overflowed messages into the executor.
    ///
    /// Returns the number of messages successfully reinjected. Stops early (leaving the
    /// rest for a later poll) as soon as the executor queue is full, so a wedged executor
    /// does not spin this into a claim loop. Over-age messages are dropped.
    ///
    /// Note: this lists keys in the ready namespace each call. `reload_batch_size` bounds
    /// how many are *claimed*, not how many keys are loaded into memory. Prefer keeping
    /// `max_persisted` reasonable for the storage backend.
    pub async fn reload_batch(&self, executor: &Executor) -> usize {
        // Recover anything left mid-claim by a crashed peer / prior boot first.
        let _ = self.reclaim_stale_inflight().await;

        let keys = match self.storage.list_keys(QUEUE_OVERFLOW_NS, None).await {
            Ok(keys) => keys,
            Err(e) => {
                error!("Could not list overflow messages to reload: {e}");
                return 0;
            }
        };

        if keys.is_empty() {
            return 0;
        }

        if keys.len() > LIST_SIZE_WARN_THRESHOLD {
            warn!(
                "Overflow ready namespace has {} keys; full list_keys each reload poll is expensive. Consider lowering max_persisted or draining backlog.",
                keys.len()
            );
        }

        // Keys are `{zero-padded-millis}:{id}`, so a lexical sort is oldest-first.
        let mut keys = keys;
        keys.sort();

        let now = now_secs();
        let mut reinjected = 0usize;
        let batch_limit = self.config.reload_batch_size;

        for key in keys.into_iter().take(batch_limit) {
            // Step 1: claim ready row. Only one replica receives Some(value).
            let body = match self.storage.delete(QUEUE_OVERFLOW_NS, &key).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(e) => {
                    error!("Could not claim overflow message {key}: {e}");
                    continue;
                }
            };

            // Step 2: park in inflight IMMEDIATELY so a crash from here on is recoverable.
            let inflight_blob = wrap_inflight(now, &body);
            if let Err(e) = self
                .storage
                .insert(
                    QUEUE_OVERFLOW_INFLIGHT_NS.to_string(),
                    key.clone(),
                    inflight_blob,
                )
                .await
            {
                error!(
                    "Could not park claimed overflow {key} in inflight: {e}; restoring to ready"
                );
                if let Err(e2) = self
                    .storage
                    .insert(QUEUE_OVERFLOW_NS.to_string(), key.clone(), body)
                    .await
                {
                    error!(
                        "Failed to restore overflow {key} after inflight park failure: {e2}; message may be lost"
                    );
                    self.saturating_dec();
                }
                continue;
            }

            // Drop messages that have outlived their max age (already claimed; just ack).
            if let Some(created) = millis_from_key(&key) {
                let age_secs = now.saturating_sub(created / 1000);
                if age_secs > self.config.max_message_age_secs {
                    warn!("Dropping overflow message {key}: age {age_secs}s exceeds limit");
                    self.ack_inflight(&key).await;
                    self.saturating_dec();
                    continue;
                }
            } else {
                // Unparseable key: drop rather than immortalize.
                warn!("Dropping overflow message with unparseable key {key}");
                self.ack_inflight(&key).await;
                self.saturating_dec();
                continue;
            }

            let message = match decode_message(&body) {
                Ok(message) => message,
                Err(e) => {
                    error!("Could not decode overflow message {key}: {e}; dropping");
                    self.ack_inflight(&key).await;
                    self.saturating_dec();
                    continue;
                }
            };

            match executor.execute_webhook_message(message) {
                Ok(()) => {
                    self.ack_inflight(&key).await;
                    self.saturating_dec();
                    reinjected += 1;
                }
                Err(TrySendError::Full(message)) => {
                    // No capacity. Put original bytes back under the ORIGINAL key so age
                    // is preserved, then stop: continuing would thrash against a full queue.
                    self.return_to_ready(&key, &body, &message).await;
                    break;
                }
                Err(TrySendError::Disconnected(message)) => {
                    // Executor is going away. Must NOT drop the claimed message.
                    error!(
                        "Executor channel disconnected while reloading overflow {key}; returning to ready"
                    );
                    self.return_to_ready(&key, &body, &message).await;
                    break;
                }
            }
        }

        if reinjected > 0 {
            info!("Reinjected {reinjected} overflow message(s) into the executor");
        }
        reinjected
    }

    /// Delete the inflight lease for `key` (ack after successful enqueue or drop).
    async fn ack_inflight(&self, key: &str) {
        if let Err(e) = self.storage.delete(QUEUE_OVERFLOW_INFLIGHT_NS, key).await {
            // Worst case: lease expires and we reinject a duplicate (at-least-once).
            error!("Could not ack inflight overflow {key}: {e}; may reinject after lease expiry");
        }
    }

    /// After a failed reinject, restore the original ready bytes and drop the inflight
    /// lease. Falls back to re-encoding `message` if the raw body restore fails.
    async fn return_to_ready(&self, key: &str, body: &[u8], message: &Message) {
        match self
            .storage
            .insert(QUEUE_OVERFLOW_NS.to_string(), key.to_string(), body.to_vec())
            .await
        {
            Ok(_) => {
                self.ack_inflight(key).await;
            }
            Err(e) => {
                error!(
                    "Failed to restore overflow {key} to ready with original bytes: {e}; trying re-encode"
                );
                self.ack_inflight(key).await;
                // Re-encode path bumps accounting carefully via persist_with_key restore.
                if self
                    .persist_with_key(message, true, Some(key.to_string()))
                    .await
                    != PersistOutcome::Persisted
                {
                    error!(
                        "Failed to re-persist unreinjected overflow message {}; it may be lost",
                        message.id
                    );
                    self.saturating_dec();
                }
            }
        }
    }

    fn saturating_dec(&self) {
        let mut cur = self.count.load(Ordering::Relaxed);
        loop {
            let next = cur.saturating_sub(1);
            match self
                .count
                .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }
}

/// Build the storage key for an overflow message. Millis are zero-padded to 20 digits so
/// that a lexical (string) sort of keys is also a chronological sort.
fn overflow_key(millis: u128, id: &str) -> String {
    format!("{millis:020}:{id}")
}

/// Extract the creation-time millis from an overflow key, if it parses.
fn millis_from_key(key: &str) -> Option<u64> {
    key.split(':').next()?.parse::<u64>().ok()
}

/// Encode a message to the on-disk format: gzip(json StoredMessageV1).
fn encode_message(message: &Message) -> Result<Vec<u8>, String> {
    let envelope = StoredMessageV1::from_message(message);
    let serialized =
        serde_json::to_vec(&envelope).map_err(|e| format!("serialize envelope: {e}"))?;
    compress(&serialized).map_err(|e| format!("compress: {e}"))
}

/// Decode on-disk bytes. Accepts the v1 base64 envelope and falls back to legacy
/// `serde_json::Message` payloads so an in-place upgrade does not strand rows.
fn decode_message(bytes: &[u8]) -> Result<Message, String> {
    let decompressed = decompress(bytes).map_err(|e| format!("decompress: {e}"))?;

    if let Ok(env) = serde_json::from_slice::<StoredMessageV1>(&decompressed) {
        if env.v == STORED_MESSAGE_VERSION {
            return env.into_message();
        }
    }

    // Legacy: direct Message JSON (array-of-bytes data field).
    serde_json::from_slice::<Message>(&decompressed)
        .map_err(|e| format!("deserialize legacy or v1 message: {e}"))
}

/// Inflight blob: 8-byte big-endian claimed_at_secs || original ready body.
fn wrap_inflight(claimed_at_secs: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&claimed_at_secs.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn unwrap_inflight(blob: &[u8]) -> Option<(u64, Vec<u8>)> {
    if blob.len() < 8 {
        return None;
    }
    let mut ts = [0u8; 8];
    ts.copy_from_slice(&blob[..8]);
    let claimed_at = u64::from_be_bytes(ts);
    Some((claimed_at, blob[8..].to_vec()))
}

fn compress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

fn decompress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Offer a message that could not be enqueued to the spill channel (non-blocking).
///
/// Returns `true` if the message was accepted onto the spill path (persistence is async).
/// Returns `false` if overflow is disabled or the spill backlog itself is saturated /
/// closed — the message is then dropped and the caller should treat it as lost.
pub fn offer_to_spill(
    message: Message,
    log_type: &str,
    spill_sender: &Option<tokio::sync::mpsc::Sender<Message>>,
) -> bool {
    match spill_sender {
        Some(tx) => match tx.try_send(message) {
            Ok(()) => {
                debug!("Queue Full! [{log_type}] message routed to durable overflow");
                true
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                error!(
                    "Queue Full AND overflow spill backlog full! [{log_type}] log dropped!"
                );
                false
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                error!(
                    "Queue Full and overflow spill channel closed! [{log_type}] log dropped!"
                );
                false
            }
        },
        None => {
            error!("Queue Full! [{log_type}] log dropped!");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plaid_stl::messages::{LogSource, LogbacksAllowed};

    #[test]
    fn key_roundtrips_and_sorts_chronologically() {
        let early = overflow_key(1000, "aaa");
        let late = overflow_key(2000, "bbb");
        assert!(early < late, "older key must sort before newer key");
        assert_eq!(millis_from_key(&early), Some(1000));
        assert_eq!(millis_from_key(&late), Some(2000));
    }

    #[test]
    fn key_padding_survives_magnitude_change() {
        let smaller = overflow_key(9_999_999_999_999, "a");
        let larger = overflow_key(10_000_000_000_000, "b");
        assert!(smaller < larger);
    }

    #[test]
    fn millis_from_malformed_key_is_none() {
        assert_eq!(millis_from_key("not-a-number:id"), None);
        assert_eq!(millis_from_key(""), None);
    }

    #[test]
    fn compress_decompress_roundtrip() {
        let original = b"the quick brown fox jumps over the lazy dog".repeat(100);
        let compressed = compress(&original).unwrap();
        let restored = decompress(&compressed).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn millis_parses_even_when_id_contains_colon() {
        let key = overflow_key(1234, "id:with:colons");
        assert_eq!(millis_from_key(&key), Some(1234));
    }

    #[test]
    fn inflight_wrap_roundtrip() {
        let body = b"compressed-payload".to_vec();
        let wrapped = wrap_inflight(1_700_000_000, &body);
        let (ts, got) = unwrap_inflight(&wrapped).unwrap();
        assert_eq!(ts, 1_700_000_000);
        assert_eq!(got, body);
    }

    #[test]
    fn envelope_roundtrip_preserves_binary_data() {
        let message = test_message_with_data(vec![0, 1, 2, 255, 128]);
        let encoded = encode_message(&message).unwrap();
        let restored = decode_message(&encoded).unwrap();
        assert_eq!(restored.id, message.id);
        assert_eq!(restored.data, message.data);
        assert_eq!(restored.type_, message.type_);
    }

    #[test]
    fn envelope_is_much_smaller_than_legacy_json_array_for_binary() {
        // 50 KiB of binary: legacy JSON array-of-bytes is enormous; v1+gzip should fit
        // comfortably under the DynamoDB item guard.
        let message = test_message_with_data(vec![0xABu8; 50 * 1024]);
        let encoded = encode_message(&message).unwrap();
        assert!(
            encoded.len() < MAX_ITEM_BYTES,
            "encoded {} bytes should be under limit",
            encoded.len()
        );

        // Legacy path for comparison: raw Message JSON then gzip.
        let legacy = compress(&serde_json::to_vec(&message).unwrap()).unwrap();
        assert!(
            encoded.len() < legacy.len(),
            "v1 envelope ({}) should beat legacy json-array ({})",
            encoded.len(),
            legacy.len()
        );
    }

    fn test_message() -> Message {
        test_message_with_data(b"hello world".to_vec())
    }

    fn test_message_with_data(data: Vec<u8>) -> Message {
        Message::new(
            "test_type".to_string(),
            data,
            LogSource::WebhookPost("test".to_string()),
            LogbacksAllowed::Limited(0),
        )
    }

    #[tokio::test]
    async fn persist_then_decode_roundtrips_message() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage.clone(), QueueOverflowConfig::default()).await;
        let message = test_message();
        let id = message.id.clone();

        assert_eq!(store.persist(&message).await, PersistOutcome::Persisted);

        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        assert_eq!(keys.len(), 1);
        let value = storage
            .delete(QUEUE_OVERFLOW_NS, &keys[0])
            .await
            .unwrap()
            .unwrap();
        let restored = decode_message(&value).unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.data, b"hello world");
    }

    #[tokio::test]
    async fn get_mode_message_is_not_persisted() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage.clone(), QueueOverflowConfig::default()).await;
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let mut message = test_message();
        message.response_sender = Some(tx);

        assert_eq!(
            store.persist(&message).await,
            PersistOutcome::NotReplayable
        );
        assert!(storage
            .list_keys(QUEUE_OVERFLOW_NS, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn cap_is_enforced_then_bypassed_by_forced_until_hard_cap() {
        let storage = Arc::new(Storage::new_in_memory());
        let cfg = QueueOverflowConfig {
            max_persisted: 1,
            ..QueueOverflowConfig::default()
        };
        let store = OverflowStore::new(storage.clone(), cfg).await;

        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);
        assert_eq!(
            store.persist(&test_message()).await,
            PersistOutcome::CapExceeded
        );
        // Forced bypasses soft cap...
        assert_eq!(
            store.persist_forced(&test_message()).await,
            PersistOutcome::Persisted
        );
        // ...but hard cap (2x) still binds: count is 2, hard is 2, so next forced fails.
        assert_eq!(
            store.persist_forced(&test_message()).await,
            PersistOutcome::CapExceeded
        );
    }

    #[tokio::test]
    async fn saturating_dec_never_wraps_count() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage, QueueOverflowConfig::default()).await;
        assert_eq!(store.approximate_count(), 0);
        // Simulate claiming messages we never counted (peer-written rows).
        store.saturating_dec();
        store.saturating_dec();
        assert_eq!(store.approximate_count(), 0);
        // Cap must still allow persists after underflow attempts.
        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);
    }

    #[tokio::test]
    async fn claim_parks_inflight_then_ack_clears_it() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage.clone(), QueueOverflowConfig::default()).await;
        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);

        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        let key = keys[0].clone();
        let body = storage
            .delete(QUEUE_OVERFLOW_NS, &key)
            .await
            .unwrap()
            .unwrap();
        storage
            .insert(
                QUEUE_OVERFLOW_INFLIGHT_NS.to_string(),
                key.clone(),
                wrap_inflight(now_secs(), &body),
            )
            .await
            .unwrap();

        assert!(storage
            .list_keys(QUEUE_OVERFLOW_NS, None)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            storage
                .list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None)
                .await
                .unwrap()
                .len(),
            1
        );

        store.ack_inflight(&key).await;
        assert!(storage
            .list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn stale_inflight_is_reclaimed_to_ready() {
        let storage = Arc::new(Storage::new_in_memory());
        let cfg = QueueOverflowConfig {
            claim_lease_secs: 1,
            ..QueueOverflowConfig::default()
        };
        let store = OverflowStore::new(storage.clone(), cfg).await;
        let message = test_message();
        let id = message.id.clone();
        assert_eq!(store.persist(&message).await, PersistOutcome::Persisted);

        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        let key = keys[0].clone();
        let body = storage
            .delete(QUEUE_OVERFLOW_NS, &key)
            .await
            .unwrap()
            .unwrap();
        // Claimed far in the past → lease expired.
        storage
            .insert(
                QUEUE_OVERFLOW_INFLIGHT_NS.to_string(),
                key.clone(),
                wrap_inflight(1, &body),
            )
            .await
            .unwrap();

        let n = store.reclaim_stale_inflight().await;
        assert_eq!(n, 1);
        assert!(storage
            .list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None)
            .await
            .unwrap()
            .is_empty());
        let ready = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        assert_eq!(ready.len(), 1);
        let restored = decode_message(
            &storage
                .get(QUEUE_OVERFLOW_NS, &ready[0])
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.id, id);
    }

    #[tokio::test]
    async fn fresh_inflight_is_not_reclaimed() {
        let storage = Arc::new(Storage::new_in_memory());
        let cfg = QueueOverflowConfig {
            claim_lease_secs: 3600,
            ..QueueOverflowConfig::default()
        };
        let store = OverflowStore::new(storage.clone(), cfg).await;
        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);

        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        let key = keys[0].clone();
        let body = storage
            .delete(QUEUE_OVERFLOW_NS, &key)
            .await
            .unwrap()
            .unwrap();
        storage
            .insert(
                QUEUE_OVERFLOW_INFLIGHT_NS.to_string(),
                key,
                wrap_inflight(now_secs(), &body),
            )
            .await
            .unwrap();

        assert_eq!(store.reclaim_stale_inflight().await, 0);
        assert_eq!(
            storage
                .list_keys(QUEUE_OVERFLOW_INFLIGHT_NS, None)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn reap_expired_removes_old_ready_messages() {
        let storage = Arc::new(Storage::new_in_memory());
        let cfg = QueueOverflowConfig {
            max_message_age_secs: 1,
            ..QueueOverflowConfig::default()
        };
        let store = OverflowStore::new(storage.clone(), cfg).await;

        // Insert directly with an ancient key so age check fires.
        let message = test_message();
        let encoded = encode_message(&message).unwrap();
        let ancient_key = overflow_key(1_000, &message.id); // year ~1970
        storage
            .insert(QUEUE_OVERFLOW_NS.to_string(), ancient_key, encoded)
            .await
            .unwrap();
        // Manually bump count to match (persist path would have).
        store.count.store(1, Ordering::Relaxed);

        let reaped = store.reap_expired().await;
        assert_eq!(reaped, 1);
        assert!(storage
            .list_keys(QUEUE_OVERFLOW_NS, None)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(store.approximate_count(), 0);
    }

    #[tokio::test]
    async fn legacy_json_message_still_decodes() {
        let message = test_message();
        let legacy = compress(&serde_json::to_vec(&message).unwrap()).unwrap();
        let restored = decode_message(&legacy).unwrap();
        assert_eq!(restored.id, message.id);
        assert_eq!(restored.data, message.data);
    }

    #[tokio::test]
    async fn concurrent_delete_claim_only_one_wins() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage.clone(), QueueOverflowConfig::default()).await;
        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);
        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        let key = keys[0].clone();

        let a = storage.delete(QUEUE_OVERFLOW_NS, &key).await.unwrap();
        let b = storage.delete(QUEUE_OVERFLOW_NS, &key).await.unwrap();
        assert!(a.is_some());
        assert!(b.is_none());
    }
}
