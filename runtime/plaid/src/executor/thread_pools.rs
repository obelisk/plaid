use std::collections::HashMap;
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, SendError, Sender, TrySendError};

use crate::config::ExecutorConfig;

use super::Message;

/// A pool of threads to process logs
#[derive(Clone)]
pub struct ThreadPool {
    pub num_threads: u8,
    pub sender: Sender<Message>,
    pub receiver: Receiver<Message>,
}

impl ThreadPool {
    /// Create a new thread pool with the given number of threads, operating
    /// on a channel with the given size limit.
    pub fn new(num_threads: u8, queue_size: usize) -> Self {
        let (sender, receiver) = bounded(queue_size);
        ThreadPool {
            num_threads,
            sender,
            receiver,
        }
    }
}

/// A routing-aware sender for messages entering the executor. Every data
/// generator (interval jobs, logbacks, Github, Okta, SQS, WebSockets) should
/// hold one of these instead of a raw `Sender<Message>` so that messages are
/// always routed to the pool dedicated to their log type, when one exists.
#[derive(Clone)]
pub struct MessageSender {
    general_sender: Sender<Message>,
    dedicated_senders: Arc<HashMap<String, Sender<Message>>>,
}

impl MessageSender {
    pub fn new(
        general_sender: Sender<Message>,
        dedicated_senders: HashMap<String, Sender<Message>>,
    ) -> Self {
        Self {
            general_sender,
            dedicated_senders: Arc::new(dedicated_senders),
        }
    }

    /// The sender for the pool dedicated to `log_type`, if one exists, or the
    /// general pool otherwise.
    fn sender_for(&self, log_type: &str) -> &Sender<Message> {
        match self.dedicated_senders.get(log_type) {
            Some(sender) => sender,
            None => &self.general_sender,
        }
    }

    /// Send a message to the pool dedicated to its log type, if one exists, or
    /// to the general pool otherwise. Blocks if the destination queue is full.
    pub fn send(&self, message: Message) -> Result<(), SendError<Message>> {
        self.sender_for(&message.type_).send(message)
    }

    /// Try to send a message to the pool dedicated to its log type, if one
    /// exists, or to the general pool otherwise.
    pub fn try_send(&self, message: Message) -> Result<(), TrySendError<Message>> {
        self.sender_for(&message.type_).try_send(message)
    }
}

/// A struct that keeps track of all Plaid's thread pools
#[derive(Clone)]
pub struct ExecutionThreadPools {
    /// Thread pool for general processing, i.e., for processing logs
    /// which do not have a dedicated thread pool.
    pub general_pool: ThreadPool,
    /// Thread pools dedicated to specific log types.
    /// Mapping { log_type --> thread_pool }
    pub dedicated_pools: HashMap<String, ThreadPool>,
}

impl ExecutionThreadPools {
    /// Create a new ExecutionThreadPools object by initializing only the thread
    /// pool for general processing. Other thread pools, if present, must be
    /// added separately by inserting into the `dedicated_pools` map.
    pub fn new(executor_config: &ExecutorConfig) -> Self {
        // If we are dedicating threads to specific log types, create their channels and add them to the map
        let dedicated_pools: HashMap<String, ThreadPool> = executor_config
            .dedicated_threads
            .iter()
            .map(|(logtype, config)| {
                let tp = ThreadPool::new(config.num_threads, config.log_queue_size);
                (logtype.clone(), tp)
            })
            .collect();

        ExecutionThreadPools {
            general_pool: ThreadPool::new(
                executor_config.execution_threads,
                executor_config.log_queue_size,
            ),
            dedicated_pools,
        }
    }

    /// A routing-aware sender that routes each message to the pool dedicated to
    /// its log type, if one exists, or to the general pool otherwise.
    pub fn message_sender(&self) -> MessageSender {
        let dedicated_senders = self
            .dedicated_pools
            .iter()
            .map(|(log_type, tp)| (log_type.clone(), tp.sender.clone()))
            .collect();
        MessageSender::new(self.general_pool.sender.clone(), dedicated_senders)
    }
}
