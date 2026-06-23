use crate::{executor::Message, storage::Storage};

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

use serde::{Deserialize, Serialize};

use std::{
    cmp::Reverse,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use std::collections::BinaryHeap;

use super::DataError;

const LOGBACK_NS: &str = "logback_internal";
const CHANNEL_CAPACITY: usize = 4096;

#[derive(Serialize, Deserialize)]
pub struct DelayedMessage {
    pub delay: u64,
    pub message: Message,
}

impl DelayedMessage {
    pub fn new(delay: u64, message: Message) -> Self {
        Self { delay, message }
    }
}

impl std::cmp::PartialEq for DelayedMessage {
    fn eq(&self, other: &Self) -> bool {
        self.delay == other.delay
    }
}

impl std::cmp::Eq for DelayedMessage {}

impl std::cmp::PartialOrd for DelayedMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.delay.partial_cmp(&other.delay)
    }
}

impl std::cmp::Ord for DelayedMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.delay.cmp(&other.delay)
    }
}

/// Persists incoming delayed logbacks to storage. Holds no `Sender<Message>` so the
/// perpetual listener task cannot keep executor ingress channels alive.
#[derive(Clone)]
pub struct DelayedLogPersister {
    receiver: Receiver<DelayedMessage>,
    storage: Arc<Storage>,
}

impl DelayedLogPersister {
    pub async fn listen_for_incoming_logs(&self) {
        while let Ok(log) = self.receiver.try_recv() {
            persist_delayed_log(&self.storage, log).await;
        }
    }

    /// Drain any delayed logbacks still in the in-memory channel into storage.
    pub async fn flush_pending(&self) {
        self.listen_for_incoming_logs().await;
    }
}

pub struct Internal {
    sender: Sender<Message>,
    internal_sender: Sender<DelayedMessage>,
    storage: Arc<Storage>,
}

impl Internal {
    pub fn new(
        log_sender: Sender<Message>,
        storage: Arc<Storage>,
    ) -> Result<(Self, DelayedLogPersister), DataError> {
        let (internal_sender, receiver) = bounded(CHANNEL_CAPACITY);

        Ok((
            Self {
                sender: log_sender,
                internal_sender,
                storage: storage.clone(),
            },
            DelayedLogPersister { receiver, storage },
        ))
    }

    pub fn get_sender(&self) -> Sender<DelayedMessage> {
        self.internal_sender.clone()
    }

    pub async fn fetch_internal_logs(&mut self) -> Result<(), String> {
        let current_time = get_time();

        // Fill the heap with the content read from the DB.
        // This ensures that modifications which are made out-of-band to the DB are
        // reflected in the heap's content.
        let mut log_heap = fill_heap_from_db(self.storage.clone())
            .await
            .map_err(|e| format!("{e:?}"))?;

        // Now the heap is reflecting the content of the DB: we can look at it
        // and see if something should be executed.

        while let Some(heap_top) = log_heap.peek() {
            let heap_top = &heap_top.0;

            if current_time < heap_top.delay {
                info!(
                    "There are no logs that have elapsed their delay. Next log is in: {} seconds",
                    heap_top.delay - current_time
                );
                break;
            }

            // safe unwrap because if the log_heap was empty, the call to `peek()` above would have returned None
            let log = log_heap.pop().unwrap();
            let log_id = log.0.message.id.clone();
            match self.sender.try_send(log.0.message) {
                Ok(_) => {
                    // The log has been sent: now we can remove it from the storage
                    match self.storage.delete(LOGBACK_NS, &log_id).await {
                        Ok(None) => {
                            error!("We tried to delete a log back message with ID {log_id} that wasn't persisted")
                        }
                        Ok(Some(_)) => (),
                        Err(e) => error!("Error removing persisted log with ID {log_id}: {e}"),
                    }
                }
                Err(TrySendError::Full(_)) => {
                    // Executor queue is full; leave the log in storage and retry next poll.
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    error!("Error while sending a logback with ID {log_id} for processing: channel disconnected");
                    break;
                }
            }
        }
        debug!("Heap Size: {}", log_heap.len());

        Ok(())
    }
}

/// Fill the log heap with the content read from the DB
async fn fill_heap_from_db(
    storage: Arc<Storage>,
) -> Result<BinaryHeap<Reverse<DelayedMessage>>, DataError> {
    let mut log_heap = BinaryHeap::new();

    let previous_logs = storage
        .fetch_all(LOGBACK_NS, None)
        .await
        .map_err(DataError::StorageError)?;

    for (key, value) in previous_logs {
        if let Some(value) = value {
            // We want to extract from the DB a message and a delay.
            let (message, delay) = match serde_json::from_slice::<DelayedMessage>(&value) {
                Ok(item) => (item.message, item.delay),
                Err(e) => {
                    warn!("Skipping log in storage system which could not be deserialized [{e}]");
                    continue;
                }
            };
            log_heap.push(Reverse(DelayedMessage { delay, message }));
        } else {
            warn!("Empty value for logback with key {key}, skipping it.");
        }
    }

    Ok(log_heap)
}

async fn persist_delayed_log(storage: &Storage, log: DelayedMessage) {
    let mut log = log;
    let current_time = get_time();
    log.delay += current_time;

    let db_item = match serde_json::to_vec(&log) {
        Ok(db_item) => db_item,
        Err(e) => {
            error!(
                "Failed to serialize DelayedMessage from {}. Error: {e}",
                log.message.source
            );
            return;
        }
    };

    if let Err(e) = storage
        .insert(LOGBACK_NS.to_string(), log.message.id.clone(), db_item)
        .await
    {
        error!(
            "Storage system could not persist delayed log from {}. Error: {e}",
            log.message.source
        );
    }
}

fn get_time() -> u64 {
    let start = SystemTime::now();
    start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}
