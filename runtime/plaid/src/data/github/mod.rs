use crate::apis::github::{build_github_client, Authentication};
use crate::executor::Message;
use crate::parse_duration;
use crossbeam_channel::Sender;
use lru::LruCache;
use octocrab::{self, Octocrab};
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::num::NonZeroUsize;
use std::time::Duration;
use time::OffsetDateTime;

use super::{DataGenerator, DataGeneratorLog};

/// Represents the event types GitHub will include in the response
/// to the audit log request
pub enum LogType {
    /// Returns web (non-Git) events.
    Web,
    /// Returns Git events.
    Git,
    /// Returns both web and Git events.
    All,
}

impl std::fmt::Display for LogType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogType::Web => write!(f, "web"),
            LogType::Git => write!(f, "git"),
            LogType::All => write!(f, "all"),
        }
    }
}

/// Represents a Github audit log returned from the API. For more information on audit logs,
/// see [here](https://docs.github.com/en/enterprise-cloud@latest/rest/orgs/orgs?apiVersion=2022-11-28#get-the-audit-log-for-an-organization)
#[derive(Eq, PartialEq)]
pub struct GithubAuditLog {
    /// The time the audit log event occurred, given as a Unix timestamp
    timestamp: u64,
    /// The serialized log
    serialized_log: String,
}

impl PartialOrd for GithubAuditLog {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GithubAuditLog {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

#[derive(Deserialize)]
pub struct GithubConfig {
    /// The authentication method used when configuring the GitHub API module. More
    /// methods may be added here in the future but one variant of the enum must be defined.
    /// See the Authentication enum structure above for more details.
    authentication: Authentication,
    /// The GitHub organization that logs are fetched from
    org: String,
    /// The type of logs this data generator produces
    #[serde(deserialize_with = "parse_log_type")]
    log_type: LogType,
    /// Denotes if logs produced by this generator are allowed to initiate log backs
    #[serde(default)]
    logbacks_allowed: LogbacksAllowed,
    /// Canonicalization time, i.e., after how many seconds we can consider logs as "stable"
    #[serde(default = "default_canon_time")]
    canon_time: u64,
    /// Time to wait in between calls to the GH API.
    /// If no value is provided here, we will use a default value (1 second).
    #[serde(default = "default_sleep")]
    #[serde(deserialize_with = "parse_duration")]
    sleep_duration: Duration,
    /// Size of the LRU cache that we use to deduplicate logs
    #[serde(default = "default_lru_cache_size")]
    lru_cache_size: usize,
    /// Max number of seconds in the since..until span for pulling logs from the source
    #[serde(default = "default_since_until")]
    max_since_until: u64,
    /// Max number of seconds for the look-back window
    #[serde(default = "default_max_catchup")]
    max_catchup: u64,
}

impl GithubConfig {
    /// Create a new instance of a `GithubConfig`
    pub fn new(authentication: Authentication, org: String, log_type: LogType) -> Self {
        Self {
            authentication,
            org,
            log_type,
            logbacks_allowed: LogbacksAllowed::default(),
            canon_time: 20,
            sleep_duration: Duration::from_millis(1000),
            lru_cache_size: default_lru_cache_size(),
            max_since_until: default_since_until(),
            max_catchup: default_max_catchup(),
        }
    }
}

/// This function provides the default sleep duration.
/// It is used as the default value for deserialization of the `sleep_duration` field,
/// of `GithubConfig` in the event that no value is provided.
fn default_sleep() -> Duration {
    Duration::from_millis(1000)
}

/// This function provides the default max value for the since..until time span, in seconds.
/// It is used as the default value for deserialization of the `max_since_until` field,
/// of `GithubConfig` in the event that no value is provided.
fn default_since_until() -> u64 {
    60
}

/// This function provides the default max value for the max catch-up look-back window.
fn default_max_catchup() -> u64 {
    3 * 3600 // 3 hours
}

/// This function provides the default size of the LRU cache.
fn default_lru_cache_size() -> usize {
    4096
}

fn default_canon_time() -> u64 {
    20
}

/// Custom parser for log type
fn parse_log_type<'de, D>(deserializer: D) -> Result<LogType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let log_type = String::deserialize(deserializer)?.to_lowercase();

    match log_type.as_str() {
        "web" => Ok(LogType::Web),
        "git" => Ok(LogType::Git),
        "all" => Ok(LogType::All),
        _ => Err(serde::de::Error::custom(
            "Invalid log type provided. Expected one of All, Git, Web",
        )),
    }
}

/// Represents the entire GitHub data generator set up
pub struct Github {
    /// API client
    client: Octocrab,
    /// The configuration of the generator
    config: GithubConfig,
    /// Timestamp of the last seen log we have processed
    last_seen: OffsetDateTime,
    /// The logger used to send logs to the execution system for processing
    logger: Sender<Message>,
    /// An LRU where we store the UUIDs of logs that we have already seen and sent into the logging system.
    /// This, together with some overlapping queries to the GH API, helps us ensure that all logs are processed
    /// exactly once.
    /// This LRU has a limited capacity: when this is reached, the least-recently-used item is removed to make space for a new insertion.
    /// Note: we only use the "key" part to keep track of the UUIDs we have seen. The "value" part is not used and always set to 0u32.
    seen_logs_uuid: LruCache<String, u32>,
}

impl Github {
    pub fn new(config: GithubConfig, logger: Sender<Message>) -> Self {
        let client = build_github_client(&config.authentication);
        let lru_cache_size = config.lru_cache_size;

        Self {
            config,
            client,
            last_seen: OffsetDateTime::now_utc(),
            seen_logs_uuid: LruCache::new(NonZeroUsize::new(lru_cache_size).unwrap()),
            logger,
        }
    }
}

impl DataGenerator for Github {
    // For the documentation on these methods, see the trait.

    async fn fetch_logs(
        &self,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
    ) -> Result<Vec<super::DataGeneratorLog>, ()> {
        let since = match since.format(&time::format_description::well_known::Rfc3339) {
            Ok(since) => since,
            Err(e) => {
                error!("Failed to parse 'since' datetime. Error: {e}");
                return Err(());
            }
        };
        let until = match until.format(&time::format_description::well_known::Rfc3339) {
            Ok(until) => until,
            Err(e) => {
                error!("Failed to parse 'until' datetime. Error: {e}");
                return Err(());
            }
        };

        let address = format!(
            "https://api.github.com/orgs/{}/audit-log?include={}&per_page=100&order=asc&phrase=created:{since}..{until}",
            self.config.org, self.config.log_type
        );

        let mut output_logs = vec![];
        let mut next = Some(address);

        loop {
            if let Some(address) = next {
                let response = self.client._get(&address).await.map_err(|e| {
                    error!("{}", format!("Could not get logs from GitHub: {}", e));
                })?;

                if !response.status().is_success() {
                    error!(
                        "{}",
                        format!(
                            "Call to get GitHub logs failed with code: {}",
                            response.status()
                        )
                    );
                    return Err(());
                }

                // See if there is another page of logs after this by looking at the `link` header
                // https://docs.github.com/en/enterprise-cloud@latest/rest/using-the-rest-api/using-pagination-in-the-rest-api?apiVersion=2022-11-28#using-link-headers
                next = response
                    .headers()
                    .get("link")
                    .and_then(|v| super::get_next_from_link_header(v));

                // Parse the response's body
                let body = self.client.body_to_string(response).await.map_err(|e| {
                    error!(
                        "{}",
                        format!("Failed to read body of GitHub response. Error: {e}")
                    );
                })?;

                let logs: Vec<Value> = serde_json::from_str(body.as_str()).map_err(|e| {
                    error!("{}", format!("Could not parse data from Github: {}", e));
                })?;

                if logs.is_empty() {
                    return Ok(output_logs);
                }

                // Loop over the logs we got from GitHub, parse them and add them to the growing vector
                for log in &logs {
                    let timestamp = match log.get("@timestamp") {
                        Some(v) => {
                            let Some(v) = v.as_u64() else {
                                error!("Got a log from Github without numerical @timestamp field");
                                continue;
                            };

                            v
                        }
                        None => {
                            error!("Got a log from Github without @timestamp field");
                            continue;
                        }
                    };

                    // The timestamp from GitHub is in milliseconds
                    let log_timestamp = match OffsetDateTime::from_unix_timestamp_nanos(
                        timestamp as i128 * 1_000_000,
                    ) {
                        Ok(t) => t,
                        Err(_) => {
                            error!("Couldn't parse timestamp into datetime");
                            continue;
                        }
                    };

                    let uuid = match log.get("_document_id").and_then(|v| v.as_str()) {
                        Some(id) => id,
                        None => {
                            error!("Got a GH log without ID");
                            continue;
                        }
                    };

                    // We parsed from JSON so serialization back should be safe
                    let log_bytes = serde_json::to_vec(&log).unwrap();

                    output_logs.push(DataGeneratorLog {
                        id: uuid.to_string(),
                        timestamp: log_timestamp,
                        payload: log_bytes,
                    });
                }
            } else {
                // next is None: exit the loop if there is no next page
                break;
            }
        }

        Ok(output_logs)
    }

    fn get_name(&self) -> String {
        "GitHub".to_string()
    }

    fn get_sleep_duration(&self) -> Duration {
        self.config.sleep_duration
    }

    fn get_canon_time(&self) -> u64 {
        self.config.canon_time
    }

    fn get_last_seen(&self) -> time::OffsetDateTime {
        self.last_seen
    }

    fn set_last_seen(&mut self, v: time::OffsetDateTime) {
        self.last_seen = v;
    }

    fn was_already_seen(&self, id: impl std::fmt::Display) -> bool {
        self.seen_logs_uuid.contains(&id.to_string())
    }

    fn mark_already_seen(&mut self, id: impl std::fmt::Display) {
        self.seen_logs_uuid.put(id.to_string(), 0u32);
    }

    fn send_for_processing(&self, payload: Vec<u8>) -> Result<(), ()> {
        self.logger
            .send(Message::new(
                format!("github"),
                payload,
                LogSource::Generator(Generator::Github),
                self.config.logbacks_allowed.clone(),
            ))
            .map_err(|_| ())
    }

    fn list_already_seen(&self) -> Vec<String> {
        self.seen_logs_uuid
            .iter()
            .map(|(key, _val)| key.to_string())
            .collect()
    }

    fn get_max_since_until_interval(&self) -> u64 {
        self.config.max_since_until
    }

    fn get_max_catchup_time(&self) -> u64 {
        self.config.max_catchup
    }
}
