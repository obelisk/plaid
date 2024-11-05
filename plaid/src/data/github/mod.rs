use crate::apis::github::{build_github_client, Authentication};
use crate::executor::Message;
use crossbeam_channel::Sender;
use lru::LruCache;
use octocrab::{self, Octocrab};
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// If a log at the top of the heap is this much older than a log that just came in
/// we will assume we will not see an earlier log and will ship it.
///
/// GitHub seems to find canonicalization after 20 seconds (at least on web)
/// 1000 = 1 second
const CANONICALIZATION_TIME: u64 = 20_000;

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
}

impl GithubConfig {
    /// Create a new instance of a `GithubConfig`
    pub fn new(authentication: Authentication, org: String, log_type: LogType) -> Self {
        Self {
            authentication,
            org,
            log_type,
            logbacks_allowed: LogbacksAllowed::default(),
        }
    }
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
            "Invalid log tpye provided. Expected one of All, Git, Web",
        )),
    }
}

/// Represents the entire GitHub data generator set up
pub struct Github {
    /// API client
    client: Octocrab,
    /// The configuration of the generator
    config: GithubConfig,
    /// Contains GitHub logs that are yet to be cannonicalized
    canonicalization_contents: LruCache<String, u64>,
    /// Logs awaiting to be sent to the execution system.
    canonicalization: BinaryHeap<Reverse<GithubAuditLog>>,
    /// The logger used to send logs to the execution system for processing
    logger: Sender<Message>,
}

impl Github {
    pub fn new(config: GithubConfig, logger: Sender<Message>) -> Self {
        let client = build_github_client(&config.authentication);

        Self {
            config,
            client,
            canonicalization_contents: LruCache::new(NonZeroUsize::new(512).unwrap()),
            canonicalization: BinaryHeap::new(),
            logger,
        }
    }

    /// Gets the audit log for an organization.
    ///
    /// The audit log allows organization admins to quickly review the actions performed by members of the organization.
    /// It includes details such as who performed the action, what the action was, and when it was performed.
    pub async fn fetch_audit_logs(&mut self) -> Result<(), String> {
        // Get the most recent 100 logs
        let address = format!(
            "https://api.github.com/orgs/{}/audit-log?include={}&per_page=100",
            self.config.org, self.config.log_type
        );

        let response = self
            .client
            ._get(&address)
            .await
            .map_err(|e| format!("Could not get logs from GitHub: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Call to get GitHub logs failed with code: {}",
                response.status()
            ));
        }

        let body = self
            .client
            .body_to_string(response)
            .await
            .map_err(|e| format!("Failed to read body of GitHub response. Error: {e}"))?;

        let logs: Vec<Value> = serde_json::from_str(body.as_str())
            .map_err(|e| format!("Could not parse data from Github: {}\n\n{}", e, body))?;

        let mut new_logs = 0;
        for log in logs {
            // We parsed from JSON so serialization back should be safe
            let log_str = serde_json::to_string(&log).unwrap();
            if self.canonicalization_contents.contains(&log_str) {
                continue;
            }

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

            let gh_audit_log = GithubAuditLog {
                timestamp,
                serialized_log: log_str.clone(),
            };

            self.canonicalization.push(std::cmp::Reverse(gh_audit_log));
            self.canonicalization_contents.push(log_str, timestamp);
            new_logs += 1;
        }

        let heap_time = match self.canonicalization.peek() {
            Some(x) => x.0.timestamp,
            _ => 0,
        };

        info!("Received {new_logs} new logs which are waiting for canonicalization. Next time on heap is: {heap_time}");
        // In theory, if we've added all logs above into the heap, we should go back and add more
        // until we see logs we recognize up to the capacity of the LRU cache.

        // In practice, for this to be a concern we'd need to receive more than 100 logs in the calling period
        // (about 10 seconds) which I have not seen happen

        let start = SystemTime::now();
        let current_time = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        while let Some(heap_top) = self.canonicalization.peek() {
            let heap_top = &heap_top.0;

            // If GitHub servers are ahead of us in time, this will occur
            if current_time < heap_top.timestamp {
                info!("Most recent log is from the future. Waiting for canonicalization");
                break;
            }

            // If the difference between the heap top and the current time is greater
            // than the time to wait for canonicalization, then we can send the log and
            // take it off the heap.
            if current_time - heap_top.timestamp > CANONICALIZATION_TIME {
                let log = self.canonicalization.pop().unwrap();
                if let Err(e) = self.logger.send(Message::new(
                    format!("github"),
                    log.0.serialized_log.into_bytes(),
                    LogSource::Generator(Generator::Github),
                    self.config.logbacks_allowed.clone(),
                )) {
                    error!("Failed to send GitHub log to executor. Error: {e}")
                }
            } else {
                trace!("Top of heap is: {}", heap_top.timestamp);
                break;
            }
        }

        debug!("Heap Size: {}", self.canonicalization.len());
        debug!("LRU Size: {}", self.canonicalization_contents.len());
        debug!("Current Timestamp: {}", current_time);
        Ok(())
    }
}
