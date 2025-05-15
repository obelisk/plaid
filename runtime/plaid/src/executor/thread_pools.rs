use std::collections::HashMap;

use crossbeam_channel::{bounded, Receiver, Sender};

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
}
