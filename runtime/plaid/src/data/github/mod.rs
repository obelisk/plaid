use crate::apis::github::{
    build_github_clients, classify, ApiKind, Authentication, RateLimitKind, RateLimitSnapshot,
};
use crate::apis::ApiError;
use crate::executor::{Message, MessageSender};
use crate::metrics::{LabelSet, MetricsHandle};
use crate::parse_duration;
use lru::LruCache;
use octocrab::{self, Octocrab};
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use prometheus::register_int_counter_vec_with_registry;
use prometheus::register_int_counter_with_registry;
use prometheus::{IntCounter, IntCounterVec};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

use super::{DataGenerator, DataGeneratorLog, FetchErrorStage};

struct FetchErrorLabels {
    reason: FetchErrorStage,
    kind: Option<RateLimitKind>,
}

impl FetchErrorLabels {
    fn values(&self) -> [&str; 2] {
        [
            self.reason.as_label(),
            self.kind.map_or("none", |k| k.as_str()),
        ]
    }
}

impl LabelSet for FetchErrorLabels {
    const NAMES: &'static [&'static str] = &["reason", "kind"];
}

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
    /// Time granularity for the GitHub API, in nanoseconds.
    #[serde(default = "default_time_granularity")]
    time_granularity: u64,
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
            time_granularity: default_time_granularity(),
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

fn default_time_granularity() -> u64 {
    1_000_000 // 1 millisecond in nanoseconds
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
    /// The logger used to send logs to the execution system for processing.
    /// Routes each log to the pool dedicated to its log type, when one exists.
    logger: MessageSender,
    /// An LRU where we store the UUIDs of logs that we have already seen and sent into the logging system.
    /// This, together with some overlapping queries to the GH API, helps us ensure that all logs are processed
    /// exactly once.
    /// This LRU has a limited capacity: when this is reached, the least-recently-used item is removed to make space for a new insertion.
    /// Note: we only use the "key" part to keep track of the UUIDs we have seen. The "value" part is not used and always set to 0u32.
    seen_logs_uuid: LruCache<String, u32>,
    /// Cumulative count of logs sent for processing, when metrics are enabled.
    logs_fetched: Option<IntCounter>,
    /// Cumulative count of failed fetch attempts by stage, when metrics are enabled.
    logs_fetch_errors: Option<IntCounterVec>,
}

impl Github {
    fn record_fetch_error(&self, stage: FetchErrorStage, kind: Option<RateLimitKind>) {
        if let Some(errors) = &self.logs_fetch_errors {
            let labels = FetchErrorLabels {
                reason: stage,
                kind,
            };
            errors.with_label_values(&labels.values()).inc();
        }
    }

    pub fn new(
        config: GithubConfig,
        logger: MessageSender,
        metrics: Option<Arc<MetricsHandle>>,
    ) -> Result<Self, ApiError> {
        let default_client_auth: HashMap<String, Authentication> = [(
            "gh_data_generator".to_string(),
            config.authentication.clone(),
        )]
        .into();
        let clients = build_github_clients(&default_client_auth)?;
        let client = clients.into_iter().next().unwrap().1; // we know there is at least one client because we just built it with one entry
        let lru_cache_size = match config.lru_cache_size {
            0 => {
                warn!("GitHub API: The LRU cache size must be greater than 0. Using default value of {}.", default_lru_cache_size());
                NonZeroUsize::new(default_lru_cache_size()).unwrap()
            }
            size => NonZeroUsize::new(size).unwrap(),
        };

        let logs_fetched = metrics
            .as_deref()
            .map(|handle| {
                register_int_counter_with_registry!(
                    "plaid_github_logs_fetched_total",
                    "Total number of GitHub logs sent for processing by the data generator",
                    handle.registry(),
                )
                .map_err(ApiError::MetricsRegistrationError)
            })
            .transpose()?;
        let logs_fetch_errors = metrics
            .as_deref()
            .map(|handle| {
                register_int_counter_vec_with_registry!(
                    "plaid_github_logs_fetch_errors_total",
                    "Total number of failed GitHub log fetch attempts; reason is datetime_encoding, transport, status, body_read, or response_parse; kind is none, primary, or secondary when the status classified as a rate limit",
                    FetchErrorLabels::NAMES,
                    handle.registry(),
                )
                .map_err(ApiError::MetricsRegistrationError)
            })
            .transpose()?;

        Ok(Self {
            config,
            client,
            last_seen: OffsetDateTime::now_utc(),
            seen_logs_uuid: LruCache::new(lru_cache_size),
            logger,
            logs_fetched,
            logs_fetch_errors,
        })
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
                self.record_fetch_error(FetchErrorStage::DateTimeEncoding, None);
                error!("Failed to parse 'since' datetime. Error: {e}");
                return Err(());
            }
        };
        let until = match until.format(&time::format_description::well_known::Rfc3339) {
            Ok(until) => until,
            Err(e) => {
                self.record_fetch_error(FetchErrorStage::DateTimeEncoding, None);
                error!("Failed to parse 'until' datetime. Error: {e}");
                return Err(());
            }
        };

        let address = format!(
            "/orgs/{}/audit-log?include={}&per_page=100&order=asc&phrase=created:{since}..{until}",
            self.config.org, self.config.log_type
        );

        let mut output_logs = vec![];
        let mut next = Some(address);

        loop {
            if let Some(address) = next {
                let response = self.client._get(&address).await.map_err(|e| {
                    self.record_fetch_error(FetchErrorStage::Transport, None);
                    error!("{}", format!("Could not get logs from GitHub: {}", e));
                })?;

                let status = response.status();
                if !status.is_success() {
                    let snapshot = RateLimitSnapshot::from_headers(response.headers());
                    let body = self.client.body_to_string(response).await;
                    let body_text = body.as_deref().unwrap_or("N/A");
                    error!(
                        "{}",
                        format!(
                            "Call to get GitHub logs failed with code: {status}. The body was [{body_text}]"
                        )
                    );
                    let outcome = classify(ApiKind::Rest, status, &snapshot, body.as_deref().ok());
                    self.record_fetch_error(
                        FetchErrorStage::ResponseStatus,
                        outcome.rate_limit_kind(),
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
                    self.record_fetch_error(FetchErrorStage::ResponseBody, None);
                    error!(
                        "{}",
                        format!("Failed to read body of GitHub response. Error: {e}")
                    );
                })?;

                let logs: Vec<Value> = serde_json::from_str(body.as_str()).map_err(|e| {
                    self.record_fetch_error(FetchErrorStage::ResponseParse, None);
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
            .map_err(|_| ())?;

        if let Some(counter) = &self.logs_fetched {
            counter.inc();
        }

        Ok(())
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

    fn get_time_granularity(&self) -> time::Duration {
        time::Duration::nanoseconds(self.config.time_granularity as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::MessageSender;
    use crate::metrics::{label_pairs, MetricsHandle};
    use time::{Date, Month, Time};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn fetch_error_labels_pair_names_with_values() {
        let classified = FetchErrorLabels {
            reason: FetchErrorStage::ResponseStatus,
            kind: Some(RateLimitKind::Primary),
        };
        assert_eq!(
            label_pairs(FetchErrorLabels::NAMES, &classified.values()),
            [("reason", "status"), ("kind", "primary")]
        );

        let unclassified = FetchErrorLabels {
            reason: FetchErrorStage::Transport,
            kind: None,
        };
        assert_eq!(
            label_pairs(FetchErrorLabels::NAMES, &unclassified.values()),
            [("reason", "transport"), ("kind", "none")]
        );
    }

    #[tokio::test]
    async fn duplicate_metric_registration_returns_an_error() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let handle = Arc::new(MetricsHandle::new());
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let sender = MessageSender::new(sender, HashMap::new());
        let config: GithubConfig =
            toml::from_str("authentication = {}\norg = \"test-org\"\nlog_type = \"all\"\n")
                .unwrap();
        assert!(Github::new(config, sender.clone(), Some(handle.clone())).is_ok());

        let config: GithubConfig =
            toml::from_str("authentication = {}\norg = \"test-org\"\nlog_type = \"all\"\n")
                .unwrap();
        assert!(matches!(
            Github::new(config, sender, Some(handle)),
            Err(ApiError::MetricsRegistrationError(_))
        ));
    }

    #[tokio::test]
    async fn failing_fetch_records_its_stage() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let handle = Arc::new(MetricsHandle::new());
        let config: GithubConfig =
            toml::from_str("authentication = {}\norg = \"test-org\"\nlog_type = \"all\"\n")
                .unwrap();
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let github = Github::new(
            config,
            MessageSender::new(sender, HashMap::new()),
            Some(handle.clone()),
        )
        .unwrap();

        // RFC3339 cannot encode a negative year, so the fetch fails before it
        // builds a request.
        let unencodable = OffsetDateTime::new_utc(
            Date::from_calendar_date(-1, Month::January, 1).unwrap(),
            Time::MIDNIGHT,
        );

        assert!(DataGenerator::fetch_logs(&github, unencodable, unencodable)
            .await
            .is_err());

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains(
            "plaid_github_logs_fetch_errors_total{kind=\"none\",reason=\"datetime_encoding\"} 1"
        ));
        assert!(encoded.contains("plaid_github_logs_fetched_total 0"));
    }

    #[tokio::test]
    async fn status_failure_records_its_rate_limit_kind() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let handle = Arc::new(MetricsHandle::new());
        let config: GithubConfig =
            toml::from_str("authentication = {}\norg = \"test-org\"\nlog_type = \"all\"\n")
                .unwrap();
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut github = Github::new(
            config,
            MessageSender::new(sender, HashMap::new()),
            Some(handle.clone()),
        )
        .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .expect("the data generator did not reach the local server")
                    .unwrap();
            let mut request = [0u8; 2048];
            let read = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(
                request.contains("/orgs/test-org/audit-log"),
                "request path: {request}"
            );
            let body = r#"{"message":"API rate limit exceeded"}"#;
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-ratelimit-limit: 5000\r\nx-ratelimit-remaining: 0\r\nx-ratelimit-used: 5000\r\nx-ratelimit-reset: 1760000000\r\nx-ratelimit-resource: core\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        github.client = Octocrab::builder()
            .base_uri(format!("http://{addr}"))
            .unwrap()
            .build()
            .unwrap();

        let now = OffsetDateTime::now_utc();
        let since = now - time::Duration::hours(1);
        assert!(DataGenerator::fetch_logs(&github, since, now)
            .await
            .is_err());
        server.await.unwrap();

        let encoded = handle.encode().unwrap();
        assert!(
            encoded.contains(
                "plaid_github_logs_fetch_errors_total{kind=\"primary\",reason=\"status\"} 1"
            ),
            "a 403 with exhausted quota must record a primary rate limit"
        );
    }
}
