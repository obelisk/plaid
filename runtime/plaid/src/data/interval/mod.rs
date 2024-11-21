use crossbeam_channel::Sender;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use std::{
    cmp::{max, Reverse},
    collections::{BinaryHeap, HashMap},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::executor::Message;

#[derive(Deserialize)]
/// Defines the list of interval jobs to be processed
pub struct IntervalConfig {
    /// A HashMap of job name to job config. The job's name will be included in accessory data as "job_name"
    jobs: HashMap<String, IntervalJob>,
    /// Maximum percentage of internal time to shift all jobs for better work distribution    
    #[serde(deserialize_with = "parse_splay")]
    splay: u32,
}

/// Custom parser for splay. Returns an error if a splay > 100 is given
fn parse_splay<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let splay = u32::deserialize(deserializer)?;

    if splay > 100 {
        Err(serde::de::Error::custom(
            "Invalid splay value provided. Max splay is 100",
        ))
    } else {
        Ok(splay)
    }
}

#[derive(Deserialize, Clone)]
/// Defines a interval job to be scheduled and executed
struct IntervalJob {
    /// Time (in seconds) between each execution
    interval: u64,
    /// The log type the generated message will be sent to
    log_type: String,
    /// Optional data to log to the rule
    data: Option<String>,
    /// The number of Logbacks this interval is allowed to trigger
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

#[derive(Serialize, Deserialize)]
/// Scheduled jobs are stored in a heap and contain all required data to execute and reschedule jobs
pub struct ScheduledJob {
    /// Timestamp that the job will be executed at
    execution_time: u64,
    /// Time between job executions - used in combination with execution_time to reschedule the job after execution
    interval: u64,
    /// Message to send to executor
    message: Message,
}

impl ScheduledJob {
    pub fn new(execution_time: u64, interval: u64, message: Message) -> Self {
        Self {
            execution_time,
            interval,
            message,
        }
    }
}

impl std::cmp::Eq for ScheduledJob {}

impl std::cmp::PartialEq for ScheduledJob {
    fn eq(&self, other: &Self) -> bool {
        self.execution_time == other.execution_time
    }
}

impl std::cmp::PartialOrd for ScheduledJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.execution_time.partial_cmp(&other.execution_time)
    }
}

impl std::cmp::Ord for ScheduledJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.execution_time < other.execution_time {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    }
}

/// Manages storage and sending of interval based jobs
pub struct Interval {
    /// The original configuration. Not currently included but may be useful in the future
    // config: IntervalConfig,
    /// Sends logs to executor
    sender: Sender<Message>,
    /// Stores jobs while they are waiting to be processed
    job_heap: BinaryHeap<Reverse<ScheduledJob>>,
}

impl Interval {
    pub fn new(config: IntervalConfig, log_sender: Sender<Message>) -> Self {
        let mut job_heap = BinaryHeap::new();
        let current_time = get_current_time();
        let srand = ring::rand::SystemRandom::new();

        // Initialize job heap
        // Iterates over interval job config and pushes job onto heap
        for (name, job) in config.jobs.iter() {
            // Offset the start times of our job to reduce Plaid's workload of jobs hitting at the same time
            // We calculate a job's splay by taking the configured splay as a percentage of our job interval.
            // A random number between 0 and the calculated value is then added to the job's execution time
            //
            // If the interval or configured splay is very small, it's possible that our calculated splay is 0
            // This will cause gen_range() to panic so we default to 1 if the calculated splay is 0.
            let splay = max(
                ((config.splay as f64 / 100.0) * job.interval as f64) as u64,
                1,
            );

            // Generate a random number between 0 and splay
            let mut bytes = [0u8; 8];
            // We assume we will always be able to generate randomness
            srand.fill(&mut bytes).unwrap();
            // Yes there is a slight randomness bias here but we're just calculating a splay
            // so this is not a security critical operation.
            let job_splay = u64::from_be_bytes(bytes) % splay;

            let message = ScheduledJob::new(
                job.interval + job_splay + current_time,
                job.interval,
                Message::new(
                    job.log_type.to_string(),
                    job.data.clone().unwrap_or_default().into(),
                    LogSource::Generator(Generator::Interval(name.clone())),
                    job.logbacks_allowed.clone(),
                ),
            );
            job_heap.push(Reverse(message));
        }

        Interval {
            //config,
            sender: log_sender,
            job_heap,
        }
    }

    /// Checks the heap for any jobs that are ready to be executed
    /// Returns the number of seconds until the next interval job is ready to be processed
    pub async fn fetch_interval_jobs(&mut self) -> u64 {
        let current_time = get_current_time();

        // Check if any job is ready to run again
        let mut time_until_next_execution = 0;
        while let Some(heap_top) = self.job_heap.peek() {
            let heap_top = &heap_top.0;

            // Since the heap is ordered, we only need to check the top
            // If the top isn't ready to be run again, then we can safely exit
            if current_time < heap_top.execution_time {
                debug!("There are no interval jobs that have passed their execution time. Next scheduled job is in: {} seconds", heap_top.execution_time - current_time);
                time_until_next_execution = heap_top.execution_time - current_time;
                break;
            }

            // Send job to executor
            let job = self.job_heap.pop().unwrap();
            self.sender.send(job.0.message.create_duplicate()).unwrap();

            // Reschedule the job by adding it back to the heap with an updated execution time
            let new_message = ScheduledJob {
                execution_time: job.0.interval + current_time,
                message: job.0.message.create_duplicate(),
                interval: job.0.interval,
            };

            self.job_heap.push(Reverse(new_message));
        }
        time_until_next_execution
    }
}

/// Gets the current time in seconds
fn get_current_time() -> u64 {
    let start = SystemTime::now();
    start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}
