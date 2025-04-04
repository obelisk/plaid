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

#[derive(Deserialize, Default)]
pub struct InternalConfig {}

#[derive(Serialize, Deserialize)]
pub struct DelayedMessage {
    delay: u64,
    message: Message,
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
        if self.delay < other.delay {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    }
}

pub struct Internal {
    #[allow(dead_code)]
    config: InternalConfig,
    log_heap: BinaryHeap<Reverse<DelayedMessage>>,
    sender: Sender<Message>,
    receiver: Receiver<DelayedMessage>,
    internal_sender: Sender<DelayedMessage>,
    storage: Option<Arc<Storage>>,
}

impl Internal {
    pub async fn new(
        config: InternalConfig,
        log_sender: Sender<Message>,
        storage: Option<Arc<Storage>>,
    ) -> Result<Self, DataError> {
        let (internal_sender, receiver) = bounded(4096);

        let mut log_heap = BinaryHeap::new();

        if let Some(storage) = &storage {
            let previous_logs = storage
                .fetch_all(LOGBACK_NS, None)
                .await
                .map_err(|e| DataError::StorageError(e))?;

            for (key, value) in previous_logs {
                // We want to extract from the DB a message and a delay.
                // First, we try deserializing the new format, where the DB key is a message ID, and the value is the serialized DelayedMessage
                let (message, delay) = match serde_json::from_slice::<DelayedMessage>(&value) {
                    Ok(item) => {
                        // Everything OK, we were deserializing a logback in the "new" format
                        (item.message, item.delay)
                    }
                    Err(_) => {
                        // Deserialization failed: try to deserialize in backward-compat mode,
                        // where the key was the message itself and the value was the time.
                        let message: Message = match serde_json::from_str(key.as_str()) {
                            Ok(msg) => msg,
                            Err(e) => {
                                // This deserialization failed too: we give up
                                warn!(
                                    "Skipping log in storage system which could not be deserialized [{e}]: {:X?}",
                                    key
                                );
                                continue;
                            }
                        };
                        let delay: Result<[u8; 8], _> = value.try_into();
                        let delay = match delay {
                            Ok(delay) => u64::from_be_bytes(delay),
                            Err(_) => {
                                warn!("Something went wrong while deserializing delay");
                                continue;
                            }
                        };
                        // We managed to recover a message and a delay
                        (message, delay)
                    }
                };
                log_heap.push(Reverse(DelayedMessage { delay, message }));
            }
        }

        Ok(Self {
            config,
            log_heap,
            sender: log_sender,
            receiver,
            internal_sender,
            storage,
        })
    }

    pub fn get_sender(&self) -> Sender<DelayedMessage> {
        self.internal_sender.clone()
    }

    pub async fn fetch_internal_logs(&mut self) -> Result<(), String> {
        let start = SystemTime::now();
        let current_time = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // Pull all logs off the channel, set their time, and put them on the heap.
        //
        // If persistence is available, we will also set them there in case the system
        // reboots
        while let Ok(mut log) = self.receiver.try_recv() {
            log.delay += current_time;

            if let Some(storage) = &self.storage {
                // Prepare what will be stored in the DB by serializing the DelayedMessage
                if let Ok(db_item) = serde_json::to_vec(&log) {
                    if let Err(e) = storage
                        .insert(LOGBACK_NS.to_string(), log.message.id.clone(), db_item)
                        .await
                    {
                        error!("Storage system could not persist a message: {e}");
                    }
                }
            }

            // Put the log into the in-memory heap
            self.log_heap.push(Reverse(log));
        }

        while let Some(heap_top) = self.log_heap.peek() {
            let heap_top = &heap_top.0;

            if current_time < heap_top.delay {
                info!(
                    "There are no logs that have elapsed their delay. Next log is in: {} seconds",
                    heap_top.delay - current_time
                );
                break;
            }

            let log = self.log_heap.pop().unwrap();
            if let Some(storage) = &self.storage {
                // Delete the logback from the storage because we are about to send it for processing.
                // According to the new format, the key is the ID inside the DelayedMessage's message field
                match storage.delete(LOGBACK_NS, &log.0.message.id).await {
                    Ok(None) => {
                        // We did not find this logback in the DB. There is a chance we were processing a message serialized in the old format,
                        // where the key was the message itself. Try to remove that
                        let message = serde_json::to_string(&log.0.message);
                        if let Ok(message) = message {
                            match storage.delete(LOGBACK_NS, &message).await {
                                Ok(None) => error!(
                                    "We tried to deleted a log back message that wasn't persisted"
                                ),
                                Ok(Some(_)) => (),
                                Err(e) => error!("Error removing persisted log: {e}"),
                            }
                        }
                    }
                    Ok(Some(_)) => (),
                    Err(e) => error!("Error removing persisted log: {e}"),
                }
            }
            self.sender.send(log.0.message).unwrap();
        }

        debug!("Heap Size: {}", self.log_heap.len());
        Ok(())
    }
}
