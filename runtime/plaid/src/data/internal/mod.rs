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

#[derive(Deserialize, Serialize)]
struct InternalLog {}

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

            for (message, time) in previous_logs {
                let message: Message = match serde_json::from_str(message.as_str()) {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!(
                            "Skipping log in storage system which could not be deserialized [{e}]: {:X?}",
                            message
                        );
                        continue;
                    }
                };
                let time: Result<[u8; 8], _> = time.try_into();
                if let Ok(time) = time {
                    let delay = u64::from_be_bytes(time);
                    log_heap.push(Reverse(DelayedMessage { delay, message }));
                }
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
                let time: Vec<u8> = log.delay.to_be_bytes().to_vec();
                // It is my understanding that serde is deterministic given the same structure
                // meaning that below when it comes time to remove this key serializing the same
                // struct will result in the same bytes.
                let message = serde_json::to_string(&log.message);

                if let Ok(message) = message {
                    if let Err(e) = storage.insert(LOGBACK_NS.to_string(), message, time).await {
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
                let message = serde_json::to_string(&log.0.message);
                if let Ok(message) = message {
                    match storage.delete(LOGBACK_NS, &message).await {
                        Ok(None) => {
                            error!("We tried to deleted a log back message that wasn't persisted")
                        }
                        Ok(Some(_)) => (),
                        Err(e) => error!("Error removing persisted log: {e}"),
                    }
                }
            }
            self.sender.send(log.0.message).unwrap();
        }

        debug!("Heap Size: {}", self.log_heap.len());
        Ok(())
    }
}
