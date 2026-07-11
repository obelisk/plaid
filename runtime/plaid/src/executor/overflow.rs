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
//!
//! # Burst design
//!
//! - Ingress never blocks on storage: messages go through a bounded mpsc spill channel.
//! - Multiple concurrent persist tasks drain the channel (`spill_concurrency`).
//! - Spill offers are rejected at a high watermark (HTTP 429) before the channel hard-fills.
//! - HTTP returns **202** when a message is accepted onto the spill path (not yet durable).
//! - Reload lists only `reload_batch_size` oldest keys (not the full namespace).
//! - Age reaper runs on a slower cadence than reload.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::TrySendError;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use plaid_stl::messages::{LogSource, LogbacksAllowed};
use prometheus::{
    register_histogram_vec_with_registry, register_int_counter_vec_with_registry,
    register_int_gauge_with_registry, HistogramVec, IntCounterVec, IntGauge, Registry,
};
use serde::{Deserialize, Serialize};

use crate::config::QueueOverflowConfig;
use crate::executor::{Executor, Message};
use crate::metrics::MetricsHandle;
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

/// Wire format versions.
const STORED_MESSAGE_V1: u8 = 1;
const STORED_MESSAGE_V2: u8 = 2;

/// Magic prefix for uncompressed v2 envelopes (before gzip).
const V2_MAGIC: &[u8; 4] = b"POF2";

/// Even forced persists (shutdown) refuse to grow the store beyond this multiple of
/// `max_persisted`, so a crash loop cannot fill the backend without bound.
const FORCED_CAP_MULTIPLIER: u64 = 2;

/// How long a recent persist failure keeps the spill path under elevated backpressure.
const FAILURE_BACKPRESSURE_SECS: u64 = 5;

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

    fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::CapExceeded | Self::TooLarge)
    }
}

/// Result of offering a message to the spill path (in-memory channel only).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpillOffer {
    /// Accepted onto the spill channel; persistence is still async. HTTP should return 202.
    Accepted,
    /// Rejected (disabled, pressure, full, or closed). HTTP should return 429; message dropped.
    Rejected,
}

/// Prometheus metrics for the overflow path. Optional — absent when metrics are disabled.
#[derive(Clone)]
pub struct OverflowMetrics {
    spill_depth: IntGauge,
    spill_capacity: IntGauge,
    ready_approx: IntGauge,
    inflight_approx: IntGauge,
    persist_total: IntCounterVec,
    persist_seconds: HistogramVec,
    reload_total: IntCounterVec,
    offer_total: IntCounterVec,
}

impl OverflowMetrics {
    pub fn register(handle: &MetricsHandle) -> Self {
        let registry = handle.registry();
        Self::register_with_registry(registry)
    }

    fn register_with_registry(registry: &Registry) -> Self {
        let spill_depth = register_int_gauge_with_registry!(
            "plaid_overflow_spill_depth",
            "Messages currently buffered in the in-process spill channel",
            registry
        )
        .expect("overflow spill_depth metric");

        let spill_capacity = register_int_gauge_with_registry!(
            "plaid_overflow_spill_capacity",
            "Configured capacity of the in-process spill channel",
            registry
        )
        .expect("overflow spill_capacity metric");

        let ready_approx = register_int_gauge_with_registry!(
            "plaid_overflow_ready_approx",
            "Approximate ready overflow messages counted by this process",
            registry
        )
        .expect("overflow ready_approx metric");

        let inflight_approx = register_int_gauge_with_registry!(
            "plaid_overflow_inflight_approx",
            "Approximate inflight overflow messages counted by this process",
            registry
        )
        .expect("overflow inflight_approx metric");

        let persist_total = register_int_counter_vec_with_registry!(
            "plaid_overflow_persist_total",
            "Overflow persist attempts by outcome",
            &["result"],
            registry
        )
        .expect("overflow persist_total metric");

        let persist_seconds = register_histogram_vec_with_registry!(
            "plaid_overflow_persist_seconds",
            "Wall time of a single overflow persist (encode + storage write)",
            &["result"],
            registry
        )
        .expect("overflow persist_seconds metric");

        let reload_total = register_int_counter_vec_with_registry!(
            "plaid_overflow_reload_total",
            "Overflow reload/reclaim/reap events",
            &["event"],
            registry
        )
        .expect("overflow reload_total metric");

        let offer_total = register_int_counter_vec_with_registry!(
            "plaid_overflow_offer_total",
            "Spill channel offer attempts by result",
            &["result"],
            registry
        )
        .expect("overflow offer_total metric");

        Self {
            spill_depth,
            spill_capacity,
            ready_approx,
            inflight_approx,
            persist_total,
            persist_seconds,
            reload_total,
            offer_total,
        }
    }

    fn record_persist(&self, outcome: PersistOutcome, elapsed: std::time::Duration) {
        let label = outcome.as_str();
        self.persist_total.with_label_values(&[label]).inc();
        self.persist_seconds
            .with_label_values(&[label])
            .observe(elapsed.as_secs_f64());
    }

    fn record_offer(&self, offer: SpillOffer) {
        let label = match offer {
            SpillOffer::Accepted => "accepted",
            SpillOffer::Rejected => "rejected",
        };
        self.offer_total.with_label_values(&[label]).inc();
    }

    fn record_reload_event(&self, event: &str, n: usize) {
        if n > 0 {
            self.reload_total.with_label_values(&[event]).inc_by(n as u64);
        }
    }
}

/// Shared spill-path state: depth tracking, recent failure backpressure, metrics.
#[derive(Clone)]
pub struct SpillIngress {
    tx: tokio::sync::mpsc::Sender<Message>,
    high_watermark_pct: u8,
    /// Approximate messages currently in the spill channel (inc on offer, dec on recv).
    depth: Arc<AtomicUsize>,
    capacity: usize,
    /// Unix secs of last non-success persist (for elevated backpressure).
    last_failure_secs: Arc<AtomicU64>,
    metrics: Option<OverflowMetrics>,
}

impl SpillIngress {
    pub fn new(
        tx: tokio::sync::mpsc::Sender<Message>,
        capacity: usize,
        high_watermark_pct: u8,
        metrics: Option<OverflowMetrics>,
    ) -> Self {
        if let Some(m) = &metrics {
            m.spill_capacity.set(capacity as i64);
            m.spill_depth.set(0);
        }
        Self {
            tx,
            high_watermark_pct: high_watermark_pct.min(100),
            depth: Arc::new(AtomicUsize::new(0)),
            capacity: capacity.max(1),
            last_failure_secs: Arc::new(AtomicU64::new(0)),
            metrics,
        }
    }

    pub fn sender(&self) -> tokio::sync::mpsc::Sender<Message> {
        self.tx.clone()
    }

    pub fn depth_handle(&self) -> Arc<AtomicUsize> {
        self.depth.clone()
    }

    pub fn failure_handle(&self) -> Arc<AtomicU64> {
        self.last_failure_secs.clone()
    }

    pub fn metrics(&self) -> Option<OverflowMetrics> {
        self.metrics.clone()
    }

    fn occupancy_pct(&self) -> u8 {
        let depth = self.depth.load(Ordering::Relaxed);
        ((depth.saturating_mul(100)) / self.capacity) as u8
    }

    fn under_failure_backpressure(&self) -> bool {
        let last = self.last_failure_secs.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        now_secs().saturating_sub(last) < FAILURE_BACKPRESSURE_SECS
    }

    /// Non-blocking offer onto the spill channel with high-watermark + failure backpressure.
    pub fn offer(&self, message: Message, log_type: &str) -> SpillOffer {
        // Reject oversized bodies early so they never sit in the spill buffer.
        if message.data.len() > MAX_ITEM_BYTES {
            error!(
                "Queue Full! [{log_type}] message {} body is {} bytes (over {MAX_ITEM_BYTES}); not spilled",
                message.id,
                message.data.len()
            );
            let offer = SpillOffer::Rejected;
            if let Some(m) = &self.metrics {
                m.record_offer(offer);
            }
            return offer;
        }

        let mut threshold = self.high_watermark_pct;
        if self.under_failure_backpressure() {
            // Tighten admission while storage is unhealthy so clients retry sooner.
            threshold = threshold.min(50);
        }
        if self.occupancy_pct() >= threshold {
            error!(
                "Queue Full AND overflow spill under pressure (depth≈{}/{}, threshold={}%)! [{log_type}] log dropped!",
                self.depth.load(Ordering::Relaxed),
                self.capacity,
                threshold
            );
            let offer = SpillOffer::Rejected;
            if let Some(m) = &self.metrics {
                m.record_offer(offer);
            }
            return offer;
        }

        match self.tx.try_send(message) {
            Ok(()) => {
                let d = self.depth.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(m) = &self.metrics {
                    m.spill_depth.set(d as i64);
                    m.record_offer(SpillOffer::Accepted);
                }
                debug!("Queue Full! [{log_type}] message routed to durable overflow (202 path)");
                SpillOffer::Accepted
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                error!(
                    "Queue Full AND overflow spill backlog full! [{log_type}] log dropped!"
                );
                let offer = SpillOffer::Rejected;
                if let Some(m) = &self.metrics {
                    m.record_offer(offer);
                }
                offer
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                error!(
                    "Queue Full and overflow spill channel closed! [{log_type}] log dropped!"
                );
                let offer = SpillOffer::Rejected;
                if let Some(m) = &self.metrics {
                    m.record_offer(offer);
                }
                offer
            }
        }
    }
}

/// Try the executor channel first; on Full, offer to spill if configured.
///
/// When overflow is disabled and the queue is full, falls back to blocking `send` so
/// data generators retain prior backpressure behaviour. Returns `Ok(())` if the message
/// was enqueued or accepted onto spill; `Err(())` if lost or the channel disconnected.
pub fn send_or_spill(
    sender: &crossbeam_channel::Sender<Message>,
    message: Message,
    log_type: &str,
    spill: &Option<SpillIngress>,
) -> Result<(), ()> {
    match sender.try_send(message) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(message)) => match spill {
            Some(ingress) => match ingress.offer(message, log_type) {
                SpillOffer::Accepted => Ok(()),
                SpillOffer::Rejected => Err(()),
            },
            None => {
                // Preserve historical blocking behaviour when overflow is off.
                sender.send(message).map_err(|_| ())
            }
        },
        Err(TrySendError::Disconnected(_)) => Err(()),
    }
}

/// Offer a message that could not be enqueued to the spill path (non-blocking).
///
/// Returns [`SpillOffer::Accepted`] if the message was accepted onto the spill channel
/// (persistence is still async — HTTP should use 202). Returns [`SpillOffer::Rejected`]
/// if overflow is disabled or the spill path is under pressure / full / closed.
pub fn offer_to_spill(
    message: Message,
    log_type: &str,
    spill: &Option<SpillIngress>,
) -> SpillOffer {
    match spill {
        Some(ingress) => ingress.offer(message, log_type),
        None => {
            error!("Queue Full! [{log_type}] log dropped!");
            SpillOffer::Rejected
        }
    }
}

/// Compact on-disk envelope v1: binary fields are base64 so gzip sees compressible text.
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
    #[cfg(test)]
    fn from_message(message: &Message) -> Self {
        Self {
            v: STORED_MESSAGE_V1,
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
    metrics: Option<OverflowMetrics>,
}

impl OverflowStore {
    /// Build a store and seed the approximate count from ready + inflight namespaces.
    pub async fn new(storage: Arc<Storage>, config: QueueOverflowConfig) -> Self {
        Self::new_with_metrics(storage, config, None).await
    }

    pub async fn new_with_metrics(
        storage: Arc<Storage>,
        config: QueueOverflowConfig,
        metrics: Option<OverflowMetrics>,
    ) -> Self {
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
        if let Some(m) = &metrics {
            m.ready_approx.set(ready as i64);
            m.inflight_approx.set(inflight as i64);
        }
        Self {
            storage,
            config,
            count: AtomicU64::new(count),
            metrics,
        }
    }

    pub fn config(&self) -> &QueueOverflowConfig {
        &self.config
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

    /// Record a persist outcome for spill backpressure + metrics. Call from spill workers.
    pub fn note_persist_outcome(
        &self,
        outcome: PersistOutcome,
        elapsed: std::time::Duration,
        last_failure_secs: Option<&AtomicU64>,
    ) {
        if let Some(m) = &self.metrics {
            m.record_persist(outcome, elapsed);
        }
        if outcome.is_failure() {
            if let Some(flag) = last_failure_secs {
                flag.store(now_secs(), Ordering::Relaxed);
            }
        }
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
                    self.refresh_count_gauges();
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

    fn refresh_count_gauges(&self) {
        if let Some(m) = &self.metrics {
            // We only track a combined count; split gauges use the same approximate for
            // ready and leave inflight as 0 unless reclaimed/claimed paths update them.
            m.ready_approx
                .set(self.count.load(Ordering::Relaxed) as i64);
        }
    }

    /// Drop ready messages older than `max_message_age_secs` without reinjecting them.
    /// Scans only the oldest `reload_batch_size` keys (sorted); stops at the first
    /// non-expired key so a large backlog of fresh messages is not fully listed.
    ///
    /// Returns the number of messages reaped.
    pub async fn reap_expired(&self) -> usize {
        let limit = self.config.reload_batch_size.max(1);
        let keys = match self
            .storage
            .list_keys_limited(QUEUE_OVERFLOW_NS, None, limit)
            .await
        {
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
                // Keys are oldest-first; nothing older remains in this page.
                break;
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
            if let Some(m) = &self.metrics {
                m.record_reload_event("reaped", reaped);
            }
            self.refresh_count_gauges();
        }
        reaped
    }

    /// Move expired inflight leases back to the ready namespace so they can be claimed
    /// again after a crash mid-reload. Returns how many rows were reclaimed.
    pub async fn reclaim_stale_inflight(&self) -> usize {
        let limit = self.config.reload_batch_size.max(1).saturating_mul(2);
        let keys = match self
            .storage
            .list_keys_limited(QUEUE_OVERFLOW_INFLIGHT_NS, None, limit)
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

        if reclaimed > 0 {
            if let Some(m) = &self.metrics {
                m.record_reload_event("reclaimed", reclaimed);
            }
        }
        reclaimed
    }

    /// Claim and reinject up to `reload_batch_size` overflowed messages into the executor.
    ///
    /// Returns the number of messages successfully reinjected. Stops early (leaving the
    /// rest for a later poll) as soon as the target queue is at/above the reinject high
    /// watermark or the executor rejects a send, so a wedged executor does not spin this
    /// into a claim loop.
    ///
    /// Lists only up to `reload_batch_size` oldest keys — not the full namespace.
    pub async fn reload_batch(&self, executor: &Executor) -> usize {
        // Recover anything left mid-claim by a crashed peer / prior boot first.
        let _ = self.reclaim_stale_inflight().await;

        let batch_limit = self.config.reload_batch_size.max(1);
        let keys = match self
            .storage
            .list_keys_limited(QUEUE_OVERFLOW_NS, None, batch_limit)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                error!("Could not list overflow messages to reload: {e}");
                return 0;
            }
        };

        if keys.is_empty() {
            return 0;
        }

        // Keys are `{zero-padded-millis}:{id}`; limited list is already oldest-first on
        // DynamoDB/Sled. Sort for backends that do not guarantee order (in-memory).
        let mut keys = keys;
        keys.sort();

        let now = now_secs();
        let mut reinjected = 0usize;
        let watermark = self.config.reinject_high_watermark_pct.min(100);

        for key in keys {
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

            // Avoid claim/restore thrash when the target queue is already near capacity.
            if executor.queue_occupancy_pct(&message.type_) >= watermark {
                self.return_to_ready(&key, &body, &message).await;
                break;
            }

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
            if let Some(m) = &self.metrics {
                m.record_reload_event("reinjected", reinjected);
            }
            self.refresh_count_gauges();
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

/// Encode a message to the on-disk format: gzip(v2 binary envelope).
fn encode_message(message: &Message) -> Result<Vec<u8>, String> {
    let raw = encode_v2_raw(message)?;
    compress(&raw).map_err(|e| format!("compress: {e}"))
}

/// Length-prefixed binary envelope (v2). Smaller than base64+JSON for binary payloads.
fn encode_v2_raw(message: &Message) -> Result<Vec<u8>, String> {
    let mut buf = Vec::with_capacity(64 + message.data.len());
    buf.extend_from_slice(V2_MAGIC);
    buf.push(STORED_MESSAGE_V2);
    write_str(&mut buf, &message.id);
    write_str(&mut buf, &message.type_);
    write_bytes(&mut buf, &message.data);
    write_u32(&mut buf, message.headers.len() as u32);
    for (k, v) in &message.headers {
        write_str(&mut buf, k);
        write_bytes(&mut buf, v);
    }
    write_u32(&mut buf, message.query_params.len() as u32);
    for (k, v) in &message.query_params {
        write_str(&mut buf, k);
        write_bytes(&mut buf, v);
    }
    let meta = serde_json::to_vec(&(&message.source, &message.logbacks_allowed))
        .map_err(|e| format!("serialize meta: {e}"))?;
    write_bytes(&mut buf, &meta);
    Ok(buf)
}

/// Decode on-disk bytes. Accepts v2 binary, v1 base64 envelope, and legacy Message JSON.
fn decode_message(bytes: &[u8]) -> Result<Message, String> {
    let decompressed = decompress(bytes).map_err(|e| format!("decompress: {e}"))?;

    if decompressed.starts_with(V2_MAGIC) {
        return decode_v2_raw(&decompressed);
    }

    if let Ok(env) = serde_json::from_slice::<StoredMessageV1>(&decompressed) {
        if env.v == STORED_MESSAGE_V1 {
            return env.into_message();
        }
    }

    // Legacy: direct Message JSON (array-of-bytes data field).
    serde_json::from_slice::<Message>(&decompressed)
        .map_err(|e| format!("deserialize legacy or v1 message: {e}"))
}

fn decode_v2_raw(raw: &[u8]) -> Result<Message, String> {
    if raw.len() < 5 || &raw[..4] != V2_MAGIC {
        return Err("bad v2 magic".into());
    }
    if raw[4] != STORED_MESSAGE_V2 {
        return Err(format!("unsupported v2 version {}", raw[4]));
    }
    let mut i = 5usize;
    let id = read_str(raw, &mut i)?;
    let type_ = read_str(raw, &mut i)?;
    let data = read_bytes(raw, &mut i)?;
    let header_count = read_u32(raw, &mut i)? as usize;
    let mut headers = HashMap::with_capacity(header_count);
    for _ in 0..header_count {
        let k = read_str(raw, &mut i)?;
        let v = read_bytes(raw, &mut i)?;
        headers.insert(k, v);
    }
    let qp_count = read_u32(raw, &mut i)? as usize;
    let mut query_params = HashMap::with_capacity(qp_count);
    for _ in 0..qp_count {
        let k = read_str(raw, &mut i)?;
        let v = read_bytes(raw, &mut i)?;
        query_params.insert(k, v);
    }
    let meta = read_bytes(raw, &mut i)?;
    let (source, logbacks_allowed): (LogSource, LogbacksAllowed) =
        serde_json::from_slice(&meta).map_err(|e| format!("deserialize meta: {e}"))?;
    Ok(Message {
        id,
        type_,
        data,
        headers,
        query_params,
        source,
        logbacks_allowed,
        response_sender: None,
        module: None,
    })
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_bytes(buf, s.as_bytes());
}

fn write_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    write_u32(buf, b.len() as u32);
    buf.extend_from_slice(b);
}

fn read_u32(buf: &[u8], i: &mut usize) -> Result<u32, String> {
    if *i + 4 > buf.len() {
        return Err("truncated u32".into());
    }
    let v = u32::from_le_bytes(buf[*i..*i + 4].try_into().unwrap());
    *i += 4;
    Ok(v)
}

fn read_bytes(buf: &[u8], i: &mut usize) -> Result<Vec<u8>, String> {
    let len = read_u32(buf, i)? as usize;
    if *i + len > buf.len() {
        return Err("truncated bytes".into());
    }
    let out = buf[*i..*i + len].to_vec();
    *i += len;
    Ok(out)
}

fn read_str(buf: &[u8], i: &mut usize) -> Result<String, String> {
    let bytes = read_bytes(buf, i)?;
    String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
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
    // Fast compression: spill path is latency-sensitive under burst.
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
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

/// Run concurrent spill workers until cancellation, then force-drain remaining messages.
pub async fn run_spill_workers(
    store: Arc<OverflowStore>,
    mut rx: tokio::sync::mpsc::Receiver<Message>,
    depth: Arc<AtomicUsize>,
    last_failure_secs: Arc<AtomicU64>,
    concurrency: usize,
    cancellation: tokio_util::sync::CancellationToken,
) {
    let concurrency = concurrency.max(1);
    let mut in_flight = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                // Drain buffered messages with forced persist.
                while let Ok(message) = rx.try_recv() {
                    dec_depth(&depth, store.metrics.as_ref());
                    while in_flight.len() >= concurrency {
                        let _ = in_flight.join_next().await;
                    }
                    let store = store.clone();
                    let last_failure_secs = last_failure_secs.clone();
                    in_flight.spawn(async move {
                        let start = Instant::now();
                        let id = message.id.clone();
                        let source = message.source.clone();
                        let outcome = store.persist_forced(&message).await;
                        store.note_persist_outcome(outcome, start.elapsed(), Some(&last_failure_secs));
                        outcome.log_if_not_persisted("spill-shutdown", &id, &source);
                    });
                }
                while in_flight.join_next().await.is_some() {}
                break;
            }
            maybe_msg = rx.recv() => {
                match maybe_msg {
                    Some(message) => {
                        dec_depth(&depth, store.metrics.as_ref());
                        while in_flight.len() >= concurrency {
                            let _ = in_flight.join_next().await;
                        }
                        let store = store.clone();
                        let last_failure_secs = last_failure_secs.clone();
                        in_flight.spawn(async move {
                            let start = Instant::now();
                            let id = message.id.clone();
                            let source = message.source.clone();
                            let outcome = store.persist(&message).await;
                            store.note_persist_outcome(outcome, start.elapsed(), Some(&last_failure_secs));
                            outcome.log_if_not_persisted("spill", &id, &source);
                        });
                    }
                    None => {
                        // All senders dropped; finish in-flight work.
                        while in_flight.join_next().await.is_some() {}
                        break;
                    }
                }
            }
        }
    }
    info!("Overflow spill workers shut down");
}

fn dec_depth(depth: &AtomicUsize, metrics: Option<&OverflowMetrics>) {
    let mut cur = depth.load(Ordering::Relaxed);
    loop {
        let next = cur.saturating_sub(1);
        match depth.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                if let Some(m) = metrics {
                    m.spill_depth.set(next as i64);
                }
                break;
            }
            Err(actual) => cur = actual,
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
    fn v2_envelope_is_smaller_than_v1_for_binary() {
        let message = test_message_with_data(vec![0xABu8; 50 * 1024]);
        let v2 = encode_message(&message).unwrap();
        assert!(
            v2.len() < MAX_ITEM_BYTES,
            "encoded {} bytes should be under limit",
            v2.len()
        );

        let v1_raw = serde_json::to_vec(&StoredMessageV1::from_message(&message)).unwrap();
        let v1 = compress(&v1_raw).unwrap();
        assert!(
            v2.len() <= v1.len(),
            "v2 envelope ({}) should not exceed v1 ({})",
            v2.len(),
            v1.len()
        );
    }

    #[test]
    fn legacy_v1_envelope_still_decodes() {
        let message = test_message();
        let v1_raw = serde_json::to_vec(&StoredMessageV1::from_message(&message)).unwrap();
        let encoded = compress(&v1_raw).unwrap();
        let restored = decode_message(&encoded).unwrap();
        assert_eq!(restored.id, message.id);
        assert_eq!(restored.data, message.data);
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
        assert_eq!(
            store.persist_forced(&test_message()).await,
            PersistOutcome::Persisted
        );
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
        store.saturating_dec();
        store.saturating_dec();
        assert_eq!(store.approximate_count(), 0);
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

        let message = test_message();
        let encoded = encode_message(&message).unwrap();
        let ancient_key = overflow_key(1_000, &message.id);
        storage
            .insert(QUEUE_OVERFLOW_NS.to_string(), ancient_key, encoded)
            .await
            .unwrap();
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

    #[tokio::test]
    async fn list_keys_limited_returns_oldest_first() {
        let storage = Arc::new(Storage::new_in_memory());
        for i in 0..5u128 {
            storage
                .insert(
                    QUEUE_OVERFLOW_NS.to_string(),
                    overflow_key(1000 + i, &format!("id{i}")),
                    vec![i as u8],
                )
                .await
                .unwrap();
        }
        let keys = storage
            .list_keys_limited(QUEUE_OVERFLOW_NS, None, 2)
            .await
            .unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0] < keys[1]);
        assert_eq!(millis_from_key(&keys[0]), Some(1000));
    }

    #[tokio::test]
    async fn spill_offer_rejects_at_high_watermark() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(4);
        let ingress = SpillIngress::new(tx, 4, 50, None);
        // Fill to 50% (2/4) — next offer at exactly threshold is rejected.
        assert_eq!(
            ingress.offer(test_message(), "t"),
            SpillOffer::Accepted
        );
        assert_eq!(
            ingress.offer(test_message(), "t"),
            SpillOffer::Accepted
        );
        // depth=2, capacity=4 → 50% >= 50 → reject
        assert_eq!(
            ingress.offer(test_message(), "t"),
            SpillOffer::Rejected
        );
    }
}
