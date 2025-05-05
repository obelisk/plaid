#[cfg(feature = "aws")]
mod sqs;

pub mod github;
pub mod internal;
mod interval;
mod okta;
mod websocket;

use crate::{
    executor::Message,
    logging::Logger,
    storage::{Storage, StorageError},
};

use std::{
    fmt::Display,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::Sender;

use serde::Deserialize;
use time::OffsetDateTime;

pub use self::internal::DelayedMessage;

// Configure data sources that Plaid will use fetch data itself and
// send to modules
#[derive(Deserialize)]
pub struct DataConfig {
    github: Option<github::GithubConfig>,
    okta: Option<okta::OktaConfig>,
    internal: Option<internal::InternalConfig>,
    interval: Option<interval::IntervalConfig>,
    #[cfg(feature = "aws")]
    sqs: Option<sqs::SQSConfig>,
    websocket: Option<websocket::WebSocketDataGenerator>,
}

struct DataInternal {
    github: Option<github::Github>,
    okta: Option<okta::Okta>,
    // Perhaps in the future there will be a reason to explicitly disallow
    // sending logs from one rule to another but for now we keep it always
    // enabled.
    internal: Option<internal::Internal>,
    /// Interval manages tracking and execution of jobs that are executed on a defined interval
    interval: Option<interval::Interval>,
    /// SQS pulls messages from AWS SQS queue
    #[cfg(feature = "aws")]
    sqs: Option<sqs::SQS>,
    /// Websocket manages the creation and maintenance of WebSockets that provide data to the executor
    websocket_external: Option<websocket::WebsocketGenerator>,
}

pub struct Data {}

#[derive(Debug)]
pub enum DataError {
    StorageError(StorageError),
}

impl DataInternal {
    async fn new(
        config: DataConfig,
        logger: Sender<Message>,
        storage: Option<Arc<Storage>>,
        els: Logger,
    ) -> Result<Self, DataError> {
        let github = config
            .github
            .map(|gh| github::Github::new(gh, logger.clone()));

        let okta = config
            .okta
            .map(|okta| okta::Okta::new(okta, logger.clone()));

        let internal = match config.internal {
            Some(internal) => {
                internal::Internal::new(internal, logger.clone(), storage.clone()).await
            }
            None => {
                internal::Internal::new(
                    internal::InternalConfig::default(),
                    logger.clone(),
                    storage.clone(),
                )
                .await
            }
        };

        let interval = config
            .interval
            .map(|config| interval::Interval::new(config, logger.clone()));

        let websocket_external = config
            .websocket
            .map(|ws| websocket::WebsocketGenerator::new(ws, logger.clone(), els));

        #[cfg(feature = "aws")]
        let sqs = if let Some(ct) = config.sqs {
            Some(sqs::SQS::new(ct, logger.clone()).await)
        } else {
            None
        };

        Ok(Self {
            github,
            okta,
            internal: Some(internal?),
            interval,
            #[cfg(feature = "aws")]
            sqs,
            websocket_external,
        })
    }
}

impl Data {
    pub async fn start(
        config: DataConfig,
        sender: Sender<Message>,
        storage: Option<Arc<Storage>>,
        els: Logger,
    ) -> Result<Option<Sender<DelayedMessage>>, DataError> {
        let di = DataInternal::new(config, sender, storage, els).await?;
        let handle = tokio::runtime::Handle::current();

        // Start the Github Audit task if there is one
        if let Some(mut gh) = di.github {
            handle.spawn(async move {
                loop {
                    if let Err(_) = get_and_process_dg_logs(&mut gh).await {
                        error!("GitHub Data Fetch Error")
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        // Start the Okta System Logs task if there is one
        if let Some(mut okta) = di.okta {
            handle.spawn(async move {
                loop {
                    if let Err(_) = get_and_process_dg_logs(&mut okta).await {
                        error!("Okta Data Fetch Error")
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        let internal_sender = if let Some(internal) = &di.internal {
            Some(internal.get_sender())
        } else {
            None
        };

        // Start the interval job processor
        if let Some(mut interval) = di.interval {
            handle.spawn(async move {
                loop {
                    let time_until_next_execution = interval.fetch_interval_jobs().await;
                    tokio::time::sleep(Duration::from_secs(time_until_next_execution)).await;
                }
            });
        }

        // Start the internal log processor. This doesn't need to be a tokio task,
        // but we make it one incase we need the runtime in the future. Perhaps it
        // will make sense to convert it to a standard thread but I don't see a benefit
        // to that now. As long as we don't block.
        if let Some(mut internal) = di.internal {
            handle.spawn(async move {
                loop {
                    if let Err(e) = internal.fetch_internal_logs().await {
                        error!("Internal Data Fetch Error: {:?}", e)
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        // Start the SQS task if there is one
        #[cfg(feature = "aws")]
        if let Some(mut ct) = di.sqs {
            handle.spawn(async move {
                loop {
                    let _ = ct.drain_queue().await;

                    tokio::time::sleep(Duration::from_secs(ct.config.sleep_duration)).await;
                }
            });
        }

        if let Some(websocket) = di.websocket_external {
            handle.spawn(async move {
                websocket.start().await;
            });
        }

        info!("Started Data Generators");
        Ok(internal_sender)
    }
}

/// Represents a generic log produced by a data generator
pub struct DataGeneratorLog {
    /// The unique ID for this log, as returned by the data source (e.g., GH, Okta)
    pub id: String,
    /// The timestamp at which this log was produced, as returned by the data source (e.g., GH, Okta).
    pub timestamp: OffsetDateTime,
    /// The payload for this log
    pub payload: Vec<u8>,
}

/// This is what data generators have in common
#[allow(async_fn_in_trait)]
pub trait DataGenerator {
    /// Fetch from the source some logs that were produced between `since` and `until`.
    ///
    /// Note: this function does not necessarily return _all_ logs that were produced in that time frame,
    /// so one might need to call this multiple times across different pages returned by the source.
    async fn fetch_logs(
        &self,
        since: OffsetDateTime,
        until: OffsetDateTime,
    ) -> Result<Vec<DataGeneratorLog>, ()>;

    /// Get a name for the data generator (useful e.g., for logging)
    fn get_name(&self) -> String;

    /// Get the duration (in milliseconds) the thread will sleep for, after fetching a page of logs
    fn get_sleep_duration(&self) -> u64;

    /// Get the number of seconds after which we assume the external API we are querying for logs
    /// reaches stability. This means that we assume all events that have happened at least these many
    /// seconds ago are now reflected in the logs returned by the API and nothing will change.
    fn get_canon_time(&self) -> u64;

    /// Get the datetime of the last (i.e., more recent) log we have seen
    fn get_last_seen(&self) -> OffsetDateTime;

    /// Set the datetime of the last (i.e., more recent) log we have seen
    fn set_last_seen(&mut self, v: OffsetDateTime);

    /// Check if a log with this ID was already seen before
    fn was_already_seen(&self, id: impl Display) -> bool;

    /// Mark a log with this ID as "already seen"
    fn mark_already_seen(&mut self, id: impl Display);

    /// Forward the payload to the channels for processing
    fn send_for_processing(&self, payload: Vec<u8>);
}

/// Get the system time in seconds from the Epoch
fn get_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Get logs from a data generator, one page at a time, and send them to rules for processing.
/// Internally, this method handles making overlapping queries and logs de-duplication.
pub async fn get_and_process_dg_logs(mut dg: impl DataGenerator) -> Result<(), ()> {
    let sleep_duration = Duration::from_millis(dg.get_sleep_duration());

    loop {
        // Get the logs until canon_time seconds ago
        let now = get_time();
        let until = now - dg.get_canon_time();
        let until = match OffsetDateTime::from_unix_timestamp(until as i64) {
            Ok(u) => u,
            Err(_) => {
                error!("Could not build 'until' parameter");
                return Err(());
            }
        };

        if until < dg.get_last_seen() {
            // We are in a strange situation. E.g., we have just booted, so
            // last_seen = now and until = now - canon_time, which makes until < last_seen.
            // This does not make sense, but it's not really an error. We return and, at some
            // point, we will run again with a 'sensible' set of parameters.
            debug!(
                "[{}] Waiting for canonicalization: {until} (until) < {} (last seen).",
                dg.get_name(),
                dg.get_last_seen(),
            );
            return Ok(());
        }

        // Get some logs that happened between `last_seen` and `until`.
        // Walk back a second from the actual value of last_seen, to account for problems
        // with time granularity. E.g., events happening in the same second could be missed.
        // Overlapping queries will prevent this problem from happening.
        // We would introduce the issue of seeing the same log multiple times, but this is handled later.
        let since = dg
            .get_last_seen()
            .saturating_sub(time::Duration::seconds(1));

        let logs = match dg.fetch_logs(since, until).await {
            Ok(logs) => logs,
            Err(_) => {
                error!("Could not get {} logs from source", dg.get_name());
                return Err(());
            }
        };

        // If we get no logs at all from the API, then we exit and we will retry later
        if logs.is_empty() {
            debug!(
                "No new {} logs since: {}",
                dg.get_name(),
                dg.get_last_seen()
            );
            return Ok(());
        }

        // Loop over the logs we got from the API and send them into the logging system

        // Keep track of how many logs we actually send for processing (and do not discard because already-seen)
        let mut sent_for_processing = 0u32;

        for log in logs {
            // Check if we have already seen this UUID:
            // - If we have, skip this log and continue
            // - If we haven't, add this log's UUID to the data structure and keep processing it
            if dg.was_already_seen(&log.id) {
                // We have already seen this log: skip it
                continue;
            }
            // We have not seen this log: add it to the cache
            dg.mark_already_seen(log.id);

            // Check if this is the latest log we've seen and update if so.
            // We'll use the new value to filter the subsequent API calls.
            //
            // We can ask the API to return the logs in ascending order so we could in theory just
            // take the last timstamp and set it as our max log time. I'm okay with doing another check here in the
            // case that the API's sorting fails to ensure that we do not miss any logs. The number of comparisions here
            // is nothing to be concerned about.
            if log.timestamp > dg.get_last_seen() {
                dg.set_last_seen(log.timestamp);
            }

            // Send log into the logging system to be processed by the rule(s)
            //
            // Eventually these errors need to bubble up so the service can shut down
            // then be restarted by an orchestration service
            dg.send_for_processing(log.payload);
            sent_for_processing += 1;
        }
        // If there have been no new logs sent for processing, we exit.
        // Exiting here will result in a 10 second wait between restarts
        if sent_for_processing == 0 {
            debug!(
                "No new {} logs since: {}. We have already seen them all",
                dg.get_name(),
                dg.get_last_seen()
            );

            // Important: we have to move the window forward. Otherwise, on the next call nothing will
            // change: with the same `since`, we will get the same logs and we will discard them all because
            // they will all be in the cache. I.e., the system will enter a deadlock.
            // To prevent this, we move `since` forward by a portion of the lookback window.
            // This ensures that, eventually, we will get some new logs.
            dg.set_last_seen(
                dg.get_last_seen()
                    .saturating_add(time::Duration::milliseconds(500)),
            );

            return Ok(());
        }
        info!(
            "Sent {sent_for_processing} {} logs for processing. Newest time seen is: {}",
            dg.get_name(),
            dg.get_last_seen()
        );

        // Wait for the specified period before making another request
        tokio::time::sleep(sleep_duration).await
    }
}
