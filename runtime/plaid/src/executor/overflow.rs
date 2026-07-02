//! Durable overflow for the execution queue.
//!
//! When enabled (via `[executor.queue_overflow]`), messages that would otherwise be
//! dropped are serialized, compressed, and written to a dedicated storage namespace so
//! they survive a queue-full burst or an ungraceful shutdown, and are replayed on a
//! later boot instead of being lost.
//!
//! Multi-replica safety: every message is a single row keyed by `{millis}:{id}`. A
//! replica claims a message by `delete`-ing its row; the storage layer returns the
//! previous value only to the caller whose delete actually removed it, so exactly one
//! replica ever reinjects a given message even when several webhook pods reload
//! concurrently.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::TrySendError;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::config::QueueOverflowConfig;
use crate::executor::{Executor, Message};
use crate::storage::Storage;

/// Storage namespace holding overflowed execution-queue messages.
pub const QUEUE_OVERFLOW_NS: &str = "queue_overflow";

/// DynamoDB caps a single item at 400 KB (key + all attributes). We refuse to persist a
/// compressed message larger than this, leaving headroom for the key and attribute
/// overhead, and log a distinct error instead of letting the backend reject the write.
const MAX_ITEM_BYTES: usize = 380 * 1024;

/// The result of trying to persist a single message to the overflow store.
#[derive(Debug, PartialEq, Eq)]
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

/// Durable overflow store for execution-queue messages.
pub struct OverflowStore {
    storage: Arc<Storage>,
    config: QueueOverflowConfig,
    /// Approximate number of messages currently persisted by *this* process. Seeded once
    /// at startup and adjusted as we persist/claim. Per-pod (not global), so with several
    /// replicas the true count can exceed `max_persisted`; it is a blast-radius bound, not
    /// exact accounting.
    count: AtomicU64,
}

impl OverflowStore {
    /// Build a store and seed the approximate count from what is already persisted.
    pub async fn new(storage: Arc<Storage>, config: QueueOverflowConfig) -> Self {
        let count = match storage.list_keys(QUEUE_OVERFLOW_NS, None).await {
            Ok(keys) => keys.len() as u64,
            Err(e) => {
                error!("Could not seed overflow count from storage: {e}. Starting from 0.");
                0
            }
        };
        info!("Overflow store initialized with {count} persisted message(s)");
        Self {
            storage,
            config,
            count: AtomicU64::new(count),
        }
    }

    /// Persist a single message, subject to the configured cap. Never blocks on the
    /// executor; only touches storage. Used on the hot path (queue-full spill).
    pub async fn persist(&self, message: &Message) -> PersistOutcome {
        self.persist_inner(message, false).await
    }

    /// Persist a single message, bypassing the cap. Used at shutdown, where dropping a
    /// message would be permanent data loss and there is no ongoing load to bound.
    pub async fn persist_forced(&self, message: &Message) -> PersistOutcome {
        self.persist_inner(message, true).await
    }

    async fn persist_inner(&self, message: &Message, bypass_cap: bool) -> PersistOutcome {
        self.persist_with_key(message, bypass_cap, None).await
    }

    /// Persist `message`. If `existing_key` is given (a message being put back after a
    /// failed reinject), reuse it so the original creation time, and therefore the age
    /// used by the reaper, is preserved. Otherwise mint a fresh timestamped key.
    async fn persist_with_key(
        &self,
        message: &Message,
        bypass_cap: bool,
        existing_key: Option<String>,
    ) -> PersistOutcome {
        // GET-mode messages block an HTTP client on a response channel that cannot be
        // serialized or reconstructed on reload, so replaying them is pointless.
        if message.response_sender.is_some() {
            return PersistOutcome::NotReplayable;
        }

        if !bypass_cap && self.count.load(Ordering::Relaxed) >= self.config.max_persisted {
            return PersistOutcome::CapExceeded;
        }

        let serialized = match serde_json::to_vec(message) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!(
                    "Failed to serialize overflow message {} from {}: {e}",
                    message.id, message.source
                );
                return PersistOutcome::Failed;
            }
        };

        let compressed = match compress(&serialized) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to compress overflow message {}: {e}", message.id);
                return PersistOutcome::Failed;
            }
        };

        if compressed.len() > MAX_ITEM_BYTES {
            error!(
                "Overflow message {} from {} is {} bytes compressed, over the {MAX_ITEM_BYTES} byte item limit; dropping to avoid a storage rejection",
                message.id,
                message.source,
                compressed.len()
            );
            return PersistOutcome::TooLarge;
        }

        let key = existing_key.unwrap_or_else(|| overflow_key(now_millis(), &message.id));
        match self
            .storage
            .insert(QUEUE_OVERFLOW_NS.to_string(), key, compressed)
            .await
        {
            Ok(_) => {
                self.count.fetch_add(1, Ordering::Relaxed);
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

    /// Claim and reinject up to `reload_batch_size` overflowed messages into the executor.
    ///
    /// Returns the number of messages successfully reinjected. Stops early (leaving the
    /// rest for a later poll) as soon as the executor queue is full, so a wedged executor
    /// does not spin this into a delete/reinsert loop. Over-age messages are dropped.
    pub async fn reload_batch(&self, executor: &Executor) -> usize {
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

        // Keys are `{zero-padded-millis}:{id}`, so a lexical sort is oldest-first.
        let mut keys = keys;
        keys.sort();

        let now = now_secs();
        let mut reinjected = 0usize;

        for key in keys.into_iter().take(self.config.reload_batch_size) {
            // Claim the message: whoever's delete returns the value owns it. A concurrent
            // replica that already claimed it gets None here and we simply skip.
            let value = match self.storage.delete(QUEUE_OVERFLOW_NS, &key).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(e) => {
                    error!("Could not claim overflow message {key}: {e}");
                    continue;
                }
            };
            // We removed a row that we had counted; reflect that regardless of what
            // happens next (reinjected, reaped, or dropped as corrupt).
            self.count.fetch_sub(1, Ordering::Relaxed);

            // Drop messages that have outlived their max age.
            if let Some(created) = millis_from_key(&key) {
                let age_secs = now.saturating_sub(created / 1000);
                if age_secs > self.config.max_message_age_secs {
                    warn!("Dropping overflow message {key}: age {age_secs}s exceeds limit");
                    continue;
                }
            }

            let decompressed = match decompress(&value) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Could not decompress overflow message {key}: {e}; dropping");
                    continue;
                }
            };

            let message = match serde_json::from_slice::<Message>(&decompressed) {
                Ok(message) => message,
                Err(e) => {
                    error!("Could not deserialize overflow message {key}: {e}; dropping");
                    continue;
                }
            };

            match executor.execute_webhook_message(message) {
                Ok(()) => reinjected += 1,
                Err(TrySendError::Full(message)) => {
                    // No capacity right now. Put it back under its ORIGINAL key so its true
                    // age is preserved (otherwise a perpetually-full queue would keep
                    // refreshing timestamps and the age reaper would never fire), then stop:
                    // continuing would just delete/reinsert the rest against a full queue.
                    self.repersist(message, key).await;
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    error!("Executor channel disconnected while reloading overflow {key}");
                    break;
                }
            }
        }

        if reinjected > 0 {
            info!("Reinjected {reinjected} overflow message(s) into the executor");
        }
        reinjected
    }

    /// Re-persist a message we claimed but could not reinject (queue was full), reusing its
    /// original storage key so the reaper still sees its true age. Bypasses the cap: we are
    /// putting back something we just removed, not adding new load, and dropping it would be
    /// the data loss this feature exists to prevent.
    async fn repersist(&self, message: Message, original_key: String) {
        if self
            .persist_with_key(&message, true, Some(original_key))
            .await
            != PersistOutcome::Persisted
        {
            error!(
                "Failed to re-persist unreinjected overflow message {}; it may be lost",
                message.id
            );
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
        // A 13-digit and a 14-digit millis value must still sort correctly.
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
        // UUIDs don't contain ':', but be defensive: only the first segment is the time.
        let key = overflow_key(1234, "id:with:colons");
        assert_eq!(millis_from_key(&key), Some(1234));
    }

    fn test_message() -> Message {
        Message::new(
            "test_type".to_string(),
            b"hello world".to_vec(),
            LogSource::WebhookPost("test".to_string()),
            LogbacksAllowed::Limited(0),
        )
    }

    #[tokio::test]
    async fn persist_then_reload_roundtrips_message() {
        let storage = Arc::new(Storage::new_in_memory());
        let store = OverflowStore::new(storage.clone(), QueueOverflowConfig::default()).await;
        let message = test_message();
        let id = message.id.clone();

        assert_eq!(store.persist(&message).await, PersistOutcome::Persisted);

        // The message is now in storage under a single key; claim it back and verify it
        // deserializes to the same message.
        let keys = storage.list_keys(QUEUE_OVERFLOW_NS, None).await.unwrap();
        assert_eq!(keys.len(), 1);
        let value = storage
            .delete(QUEUE_OVERFLOW_NS, &keys[0])
            .await
            .unwrap()
            .unwrap();
        let restored: Message = serde_json::from_slice(&decompress(&value).unwrap()).unwrap();
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

        assert_eq!(store.persist(&message).await, PersistOutcome::NotReplayable);
        assert!(storage
            .list_keys(QUEUE_OVERFLOW_NS, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn cap_is_enforced_then_bypassed_by_forced() {
        let storage = Arc::new(Storage::new_in_memory());
        let cfg = QueueOverflowConfig {
            max_persisted: 1,
            ..QueueOverflowConfig::default()
        };
        let store = OverflowStore::new(storage.clone(), cfg).await;

        assert_eq!(store.persist(&test_message()).await, PersistOutcome::Persisted);
        // At cap now: a normal persist is rejected...
        assert_eq!(store.persist(&test_message()).await, PersistOutcome::CapExceeded);
        // ...but a forced persist (shutdown / repersist) still goes through.
        assert_eq!(
            store.persist_forced(&test_message()).await,
            PersistOutcome::Persisted
        );
    }
}
