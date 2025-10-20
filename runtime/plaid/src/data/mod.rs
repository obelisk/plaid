pub mod github;
pub mod internal;
mod interval;
mod okta;
#[cfg(feature = "aws")]
mod sqs;
mod websocket;

use crate::{
    executor::Message,
    logging::Logger,
    storage::{Storage, StorageError},
    InstanceRoles,
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

const DATA_GENERATOR_STORAGE_PREFIX: &str = "__DATA_GENERATOR";
const LAST_SEEN_KEY: &str = "last_seen";
const ALREADY_SEEN_UUIDS_KEY: &str = "already_seen_uuids";

// Configure data sources that Plaid will use fetch data itself and
// send to modules
#[derive(Deserialize)]
pub struct DataConfig {
    github: Option<github::GithubConfig>,
    okta: Option<okta::OktaConfig>,
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

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageError(e) => write!(f, "DataError | StorageError: {}", e),
        }
    }
}

impl std::error::Error for DataError {}

impl DataInternal {
    async fn new(
        config: DataConfig,
        logger: Sender<Message>,
        storage: Arc<Storage>,
        els: Logger,
    ) -> Result<Self, DataError> {
        let github = config
            .github
            .map(|gh| github::Github::new(gh, logger.clone()));

        let okta = config
            .okta
            .map(|okta| okta::Okta::new(okta, logger.clone()));

        let internal = internal::Internal::new(logger.clone(), storage.clone()).await;

        let interval = config
            .interval
            .map(|config| interval::Interval::new(config, logger.clone()));

        #[cfg(feature = "aws")]
        let sqs = if let Some(cfg) = config.sqs {
            Some(sqs::SQS::new(cfg, logger.clone()).await)
        } else {
            None
        };

        let websocket_external = config
            .websocket
            .map(|ws| websocket::WebsocketGenerator::new(ws, logger.clone(), els));

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
        storage: Arc<Storage>,
        els: Logger,
        roles: &InstanceRoles,
    ) -> Result<Option<Sender<DelayedMessage>>, DataError> {
        let di = DataInternal::new(config, sender, storage.clone(), els).await?;
        let handle = tokio::runtime::Handle::current();

        if roles.data_generators {
            // Start the Github Audit task if there is one
            if let Some(mut gh) = di.github {
                let storage_clone = storage.clone();
                // Update the DG's state from the storage: this recovers the last_seen and seen_logs_uuid from a previous run
                update_dg_from_storage(&mut gh, Some(storage_clone.clone())).await;
                handle.spawn(async move {
                    loop {
                        if let Err(_) =
                            get_and_process_dg_logs(&mut gh, Some(storage_clone.clone())).await
                        {
                            error!("GitHub Data Fetch Error")
                        }

                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                });
            }

            // Start the Okta System Logs task if there is one
            if let Some(mut okta) = di.okta {
                let storage_clone = storage.clone();
                // Update the DG's state from the storage: this recovers the last_seen and seen_logs_uuid from a previous run
                update_dg_from_storage(&mut okta, Some(storage_clone.clone())).await;
                handle.spawn(async move {
                    loop {
                        if let Err(_) =
                            get_and_process_dg_logs(&mut okta, Some(storage_clone.clone())).await
                        {
                            error!("Okta Data Fetch Error")
                        }

                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                });
            }

            if let Some(websocket) = di.websocket_external {
                handle.spawn(async move {
                    websocket.start().await;
                });
            }
        }

        if roles.interval_jobs {
            // Start the interval job processor
            if let Some(mut interval) = di.interval {
                handle.spawn(async move {
                    loop {
                        let time_until_next_execution = interval.fetch_interval_jobs().await;
                        tokio::time::sleep(Duration::from_secs(time_until_next_execution)).await;
                    }
                });
            }
        }

        let internal_sender = if let Some(internal) = &di.internal {
            Some(internal.get_sender())
        } else {
            None
        };

        // Start the SQS task if there is one
        #[cfg(feature = "aws")]
        if let Some(mut sqs) = di.sqs {
            handle.spawn(async move {
                loop {
                    if let Err(err) = sqs.drain_queue().await {
                        error!("{err}");
                    };

                    tokio::time::sleep(Duration::from_secs(sqs.config.sleep_duration)).await;
                }
            });
        }

        // Start the internal log processor. This doesn't need to be a tokio task,
        // but we make it one incase we need the runtime in the future. Perhaps it
        // will make sense to convert it to a standard thread but I don't see a benefit
        // to that now. As long as we don't block.
        let running_logbacks = roles.logbacks;
        if let Some(mut internal) = di.internal {
            handle.spawn(async move {
                loop {
                    if let Err(e) = internal.fetch_internal_logs(running_logbacks).await {
                        error!("Internal Data Fetch Error: {:?}", e)
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
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
    /// Fetch from the source all the logs that were produced between `since` and `until`.
    /// If handling pagination is needed, this is done inside this function.
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

    /// Return a list of IDs for logs that were already seen
    fn list_already_seen(&self) -> Vec<String>;

    /// Forward the payload to the channels for processing
    fn send_for_processing(&self, payload: Vec<u8>) -> Result<(), ()>;

    /// Return the max number of seconds in the since..until interval for which we want to pull logs.
    /// This helps ensure we never pull too many logs from the source, which could end up exhausting
    /// Plaid's memory and bringing down the system.
    fn get_max_since_until_interval(&self) -> u64 {
        // Default value for all DGs: can be overwritten if individual DGs implement this method differently.
        60
    }

    /// Return the max number of seconds that we will catch up missed logs for.
    /// If Plaid has been down for less than this, then we will catch up all the logs. Otherwise,
    /// we will catch up these many seconds and _lose_ older logs. This is to enforce an upper bound
    /// and avoid unpleasant edge cases where Plaid would try to catch up months of missed logs.
    fn get_max_catchup_time(&self) -> u64;
}

/// Get the system time in seconds from the Epoch
fn get_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Read a JSON-serialized `Vec<String>` from storage.
async fn read_vec_from_storage(
    storage: Option<Arc<Storage>>,
    namespace: &str,
    key: &str,
    dg_name: &str,
) -> Option<Vec<String>> {
    let storage = storage?;

    match storage.get(namespace, key).await {
        Err(e) => {
            error!("Could not read {key} for {dg_name} from storage: {e}");
            None
        }
        Ok(Some(res)) => match serde_json::from_slice(&res) {
            Ok(v) => Some(v),
            Err(e) => {
                error!("Could not parse {key} for {dg_name} from storage: {e}");
                None
            }
        },
        Ok(None) => {
            info!("Could not find {key} for {dg_name} in storage");
            None
        }
    }
}

/// Read a string from storage.
async fn read_string_from_storage(
    storage: Option<Arc<Storage>>,
    namespace: &str,
    key: &str,
    dg_name: &str,
) -> Option<String> {
    let storage = storage?;

    match storage.get(namespace, key).await {
        Err(e) => {
            error!("Could not read {key} for {dg_name} from storage: {e}");
            None
        }
        Ok(Some(res)) => match String::from_utf8(res.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                error!("Could not parse {key} (UTF-8) for {dg_name}: {e}");
                None
            }
        },
        Ok(None) => {
            info!("Could not find {key} for {dg_name} in storage");
            None
        }
    }
}

/// Get the storage namespace the DG's state is stored under.
fn get_dg_storage_namespace(dg_name: &str) -> String {
    format!("{DATA_GENERATOR_STORAGE_PREFIX}_{}", dg_name)
}

/// Update a data generator with state information fetched from the storage.
async fn update_dg_from_storage<T: DataGenerator>(dg: &mut T, storage: Option<Arc<Storage>>) {
    // Retrieve from storage information about the previous run, if present.
    // This way, we can remember what was the last log we had seen, and we can backfill
    // if Plaid was not running for some period of time.

    let storage_namespace = &get_dg_storage_namespace(&dg.get_name());

    let last_seen: Option<String> = read_string_from_storage(
        storage.clone(),
        storage_namespace,
        LAST_SEEN_KEY,
        &dg.get_name(),
    )
    .await;
    match last_seen {
        Some(ref ls) => debug!("last_seen's value is {ls}"),
        None => debug!("last_seen is None!"),
    }

    let seen_logs_uuids: Option<Vec<String>> = read_vec_from_storage(
        storage.clone(),
        storage_namespace,
        ALREADY_SEEN_UUIDS_KEY,
        &dg.get_name(),
    )
    .await;
    match seen_logs_uuids {
        Some(ref slu) => debug!("seen_logs_uuids has {} elements", slu.len()),
        None => debug!("seen_logs_uuids is None!"),
    }

    match (last_seen, seen_logs_uuids) {
        (Some(last_seen), Some(seen_logs_uuids)) => {
            // We found state, so we update our data generator with it
            let mut last_seen = match last_seen.parse::<i128>() {
                Ok(x) => OffsetDateTime::from_unix_timestamp_nanos(x).unwrap_or_else(|e| {
                    error!(
                        "Could not create OffsetDateTime for {}, defaulting to now: {e}",
                        dg.get_name()
                    );
                    OffsetDateTime::now_utc()
                }),
                Err(e) => {
                    error!(
                        "Could not create OffsetDateTime for {}, defaulting to now: {e}",
                        dg.get_name()
                    );
                    OffsetDateTime::now_utc()
                }
            };

            // Ensure we are not going too far back in time. If we are, warn and cap the look-back window.
            let now = OffsetDateTime::now_utc();
            if (now - last_seen).whole_seconds() as u64 > dg.get_max_catchup_time() {
                warn!("Trying to catch up DG logs for {} seconds, which is higher than the limit ({} seconds). We are capping the look-back window. WARNING - This likely means we are going to miss logs!", (now - last_seen).whole_seconds() as u64, dg.get_max_catchup_time());
                last_seen =
                    now.saturating_sub(time::Duration::seconds(dg.get_max_catchup_time() as i64));
            }

            dg.set_last_seen(last_seen);

            for already_seen in seen_logs_uuids {
                dg.mark_already_seen(&already_seen);
            }
        }
        (None, None) => {
            // We did not find anything in the storage, so we just move on.
            info!("No DG state found in storage: either this is the first run or the previous state was lost");
        }
        _ => {
            // One is there and one is missing: this is not normal. There is not much we can do without
            // risking to process some logs twice or encountering other strange behaviors. So we log this
            // and move on as if we had not found anything.
            error!("Error while retrieving DG state from storage: the state is inconsistent and it will be ignored, but this is not normal.");
        }
    }
}

/// Get logs from a data generator, one page at a time, and send them to rules for processing.
/// Internally, this method handles making overlapping queries and logs de-duplication.
pub async fn get_and_process_dg_logs(
    dg: &mut impl DataGenerator,
    storage: Option<Arc<Storage>>,
) -> Result<(), ()> {
    let sleep_duration = Duration::from_millis(dg.get_sleep_duration());

    let storage_namespace = &get_dg_storage_namespace(&dg.get_name());

    loop {
        // Get logs that happened since `last_seen`.
        // Walk back a second from the actual value of `last_seen`, to account for problems
        // with time granularity. E.g., events happening in the same second could be missed.
        // Overlapping queries will prevent this problem from happening.
        // We would introduce the issue of seeing the same log multiple times, but this is handled later.
        let since = dg
            .get_last_seen()
            .saturating_sub(time::Duration::seconds(1));

        // Get the logs until canon_time seconds ago
        let mut until = get_time() - dg.get_canon_time();

        // We don't want to pull too many logs at a time, so we cap the since..until time span
        until = std::cmp::min(
            until,
            since.unix_timestamp() as u64 + dg.get_max_since_until_interval(),
        );

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

            // This log is new: send it into the logging system to be processed by the rule(s)
            dg.send_for_processing(log.payload)?;
            sent_for_processing += 1;

            // Now that the message has been successfully sent, add it to the cache
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
        }
        // If there have been no new logs sent for processing, we exit.
        // Exiting here will result in a 10 second wait between restarts
        if sent_for_processing == 0 {
            debug!(
                "No new {} logs since: {}. We have already seen them all",
                dg.get_name(),
                dg.get_last_seen()
            );
            return Ok(());
        }
        // If we are here, then we sent something for processing: we update the DG's state in the storage so that,
        // in case of a reboot, we can continue from where we had left off.
        if let Some(ref storage) = storage {
            if let Err(e) = storage
                .insert(
                    storage_namespace.to_string(),
                    LAST_SEEN_KEY.to_string(),
                    dg.get_last_seen()
                        .unix_timestamp_nanos()
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                )
                .await
            {
                error!(
                    "Could not store {LAST_SEEN_KEY} for DG {}. Continuing anyway. Error: {e}",
                    dg.get_name()
                );
            }
            let already_seen = dg.list_already_seen();
            // Serialize this Vec<String>: this should never fail but, if it does,
            // we serialize an empty vector and accept the consequences.
            let already_seen = serde_json::to_vec(&already_seen)
                .unwrap_or(serde_json::to_vec(&Vec::<String>::new()).unwrap());
            if let Err(e) = storage
                .insert(
                    storage_namespace.to_string(),
                    ALREADY_SEEN_UUIDS_KEY.to_string(),
                    already_seen,
                )
                .await
            {
                error!("Could not store {ALREADY_SEEN_UUIDS_KEY} for DG {}. Continuing anyway. Error: {e}", dg.get_name());
            }
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

/// Parse the `link` header returned by GitHub or Okta and extract the URL for `next` page.
/// More info: https://docs.github.com/en/enterprise-cloud@latest/rest/using-the-rest-api/using-pagination-in-the-rest-api?apiVersion=2022-11-28#using-link-headers
/// https://developer.okta.com/docs/api/#link-header
fn get_next_from_link_header(hv: &http::HeaderValue) -> Option<String> {
    let header = hv.to_str().ok()?.to_string();
    for part in header.split(',') {
        let part = part.trim();
        if part.ends_with("rel=\"next\"") {
            if let Some(start) = part.find('<') {
                if let Some(end) = part.find('>') {
                    return Some(part[start + 1..end].to_string());
                }
            }
        }
    }
    None
}
