use chrono::Utc;
use cron::Schedule;
use crossbeam_channel::{Sender, TrySendError};
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;
use std::str::FromStr;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::executor::Message;

#[derive(Deserialize)]
/// Defines the list of interval jobs to be processed
pub struct IntervalConfig {
    /// A HashMap of job name to job config.
    #[serde(deserialize_with = "parse_jobs")]
    jobs: HashMap<String, IntervalJob>,
}

/// Custom parser for `jobs`. Returns an error if an empty jobs map is provided
fn parse_jobs<'de, D>(deserializer: D) -> Result<HashMap<String, IntervalJob>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let map = HashMap::<String, IntervalJob>::deserialize(deserializer)?;
    if map.is_empty() {
        return Err(serde::de::Error::custom("`jobs` map must not be empty"));
    }
    Ok(map)
}

#[derive(Deserialize, Clone)]
/// Defines a interval job to be scheduled and executed
struct IntervalJob {
    /// Execution schedule for the job, specified as a seven-field cron expression:
    /// `sec min hour day-of-month month day-of-week year`
    ///
    /// For example, `"0 * * * * * *"` fires at the top of every minute.
    /// See the `cron` crate documentation for details.
    #[serde(deserialize_with = "parse_schedule")]
    schedule: Schedule,
    /// The log type the generated message will be sent to
    log_type: String,
    /// Optional data to log to the rule
    data: Option<String>,
    /// The number of Logbacks this interval is allowed to trigger
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

/// Custom parser for Schedule
fn parse_schedule<'de, D>(deserializer: D) -> Result<Schedule, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let schedule_raw = String::deserialize(deserializer)?;
    Schedule::from_str(&schedule_raw)
        .map_err(|e| serde::de::Error::custom(format!("Invalid schedule provided: {e}")))
}

#[derive(Deserialize)]
/// Scheduled jobs are stored in a heap and contain all required data to execute and reschedule jobs
pub struct ScheduledJob {
    /// Timestamp that the job will be executed at
    execution_time: u64,
    /// Execution schedule for the job
    #[serde(deserialize_with = "parse_schedule")]
    schedule: Schedule,
    /// Message to send to executor
    message: Message,
}

impl ScheduledJob {
    pub fn new(execution_time: u64, schedule: Schedule, message: Message) -> Self {
        Self {
            execution_time,
            schedule,
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
        self.execution_time.cmp(&other.execution_time)
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

        // Initialize job heap
        // Iterates over interval job config and pushes job onto heap
        for (name, job) in config.jobs.iter() {
            let next_execution = job.schedule.upcoming(Utc).take(1).collect::<Vec<_>>();
            let Some(time) = next_execution.first() else {
                warn!(
                    "Execution for interval job {name} is in the past. It will not be processed."
                );
                continue;
            };

            let message = ScheduledJob::new(
                (time.timestamp_millis() / 1000) as u64,
                job.schedule.clone(),
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
            // safe unwrap because if the job_heap was empty, the call to `peek()` above would have returned None
            let job = self.job_heap.pop().unwrap();
            let _ = self
                .sender
                .try_send(job.0.message.create_duplicate())
                .inspect_err(|e| match e {
                    TrySendError::Disconnected(_) => {
                        error!("Interval job sender channel has been disconnected. Unable to send interval job message.");
                    }
                    TrySendError::Full(_) => {
                        error!(
                            "Interval job sender channel is full. Unable to send interval job message."
                        );
                    }
                });

            // Try to get next execution time. If there are no more scheduled executions for this job, we'll exit early.
            let next_execution = job.0.schedule.upcoming(Utc).take(1).collect::<Vec<_>>();
            let Some(time) = next_execution.first() else {
                continue;
            };

            // Reschedule the job by adding it back to the heap with an updated execution time
            let new_message = ScheduledJob {
                execution_time: (time.timestamp_millis() / 1000) as u64,
                message: job.0.message.create_duplicate(),
                schedule: job.0.schedule,
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
