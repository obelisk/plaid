use crate::{executor::Message, storage::Storage};

use crossbeam_channel::{bounded, Receiver, Sender};

use serde::{Deserialize, Serialize};

use std::{
    cmp::Reverse,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use std::collections::BinaryHeap;

use super::DataError;

const LOGBACK_NS: &str = "logback_internal";

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

pub struct Internal {
    sender: Sender<Message>,
    receiver: Receiver<DelayedMessage>,
    internal_sender: Sender<DelayedMessage>,
    storage: Arc<Storage>,
}

/// Fill the log heap with the content read from the DB
async fn fill_heap_from_db(
    storage: Arc<Storage>,
) -> Result<BinaryHeap<Reverse<DelayedMessage>>, DataError> {
    let mut log_heap = BinaryHeap::new();

    let previous_logs = storage
        .fetch_all(LOGBACK_NS, None)
        .await
        .map_err(|e| DataError::StorageError(e))?;

    for (key, value) in previous_logs {
        if let Some(value) = value {
            // We want to extract from the DB a message and a delay.
            let (message, delay) = match serde_json::from_slice::<DelayedMessage>(&value) {
                Ok(item) => (item.message, item.delay),
                Err(e) => {
                    warn!(
                        "Skipping log in storage system which could not be deserialized [{e}]: {:X?}",
                        key
                    );
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

impl Internal {
    pub async fn new(
        log_sender: Sender<Message>,
        storage: Arc<Storage>,
    ) -> Result<Self, DataError> {
        let (internal_sender, receiver) = bounded(4096);

        Ok(Self {
            sender: log_sender,
            receiver,
            internal_sender,
            storage,
        })
    }

    pub fn get_sender(&self) -> Sender<DelayedMessage> {
        self.internal_sender.clone()
    }

    pub async fn fetch_internal_logs(&mut self, running_logbacks: bool) -> Result<(), String> {
        let start = SystemTime::now();
        let current_time = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // First, receive everything from the channel and write to DB
        while let Ok(mut log) = self.receiver.try_recv() {
            log.delay += current_time;

            // Prepare what will be stored in the DB by serializing the DelayedMessage
            if let Ok(db_item) = serde_json::to_vec(&log) {
                if let Err(e) = self
                    .storage
                    .insert(LOGBACK_NS.to_string(), log.message.id.clone(), db_item)
                    .await
                {
                    error!("Storage system could not persist a message: {e}");
                }
            }
        }

        // If we are _not_ running logbacks, then we are done: we have sent the logbacks to the DB
        // and someone else will pick them up.
        // Instead, if we are running logbacks, then we continue by pulling from the DB and processing.

        if running_logbacks {
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

                let log = log_heap.pop().unwrap();
                let log_id = log.0.message.id.clone();
                match self.sender.send(log.0.message) {
                    Ok(_) => {
                        // The log has been sent: now we can remove it from the storage
                        match self.storage.delete(LOGBACK_NS, &log_id).await {
                            Ok(None) => {
                                error!(
                                    "We tried to delete a log back message that wasn't persisted"
                                )
                            }
                            Ok(Some(_)) => (),
                            Err(e) => error!("Error removing persisted log: {e}"),
                        }
                    }
                    Err(e) => {
                        // Something went wrong while sending the log, so we do this:
                        // - We log an error
                        // - We don't delete it from the storage
                        // - We break the while loop
                        // It's not necessary to re-add the log to the heap, because this will be
                        // re-filled on the next iteration by reading from the storage.
                        error!("Error while sending a logback for processing: {e}");
                        break;
                    }
                }
            }
            debug!("Heap Size: {}", log_heap.len());
        }

        Ok(())
    }
}
