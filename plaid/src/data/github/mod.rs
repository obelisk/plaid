use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};

use crossbeam_channel::Sender;
use reqwest::Client;
use serde::Deserialize;

use lru::LruCache;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::executor::Message;

enum LogType {
    Web,
    Git,
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

#[derive(Eq, PartialEq)]
pub struct GithubAuditLog {
    timestamp: u64,
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
    pub token: String,
    pub org: String,
    pub log_type: String,
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

pub struct Github {
    client: Client,
    config: GithubConfig,
    canonicalization_contents: LruCache<String, u64>,
    canonicalization: BinaryHeap<Reverse<GithubAuditLog>>,
    logger: Sender<Message>,
    log_type: LogType,
}

impl Github {
    pub fn new(config: GithubConfig, logger: Sender<Message>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let log_type = match config.log_type.as_str() {
            "web" => LogType::Web,
            "git" => LogType::Git,
            _ => LogType::All,
        };

        Self {
            config,
            client,
            canonicalization_contents: LruCache::new(NonZeroUsize::new(512).unwrap()),
            canonicalization: BinaryHeap::new(),
            logger,
            log_type,
        }
    }

    pub async fn fetch_audit_logs(&mut self) -> Result<(), String> {
        // Get the most recent 100 logs
        let address = format!(
            "https://api.github.com/orgs/{}/audit-log?include={}&per_page=100",
            self.config.org, self.log_type
        );

        let response = self
            .client
            .get(&address)
            .header("User-Agent", "Rust/Plaid")
            .header("Authorization", format!("token {}", self.config.token))
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| format!("Could not get logs from GitHub: {}", e))?;

        let body = response
            .text()
            .await
            .map_err(|e| format!("Could not get data from Github: {}", e))?;
        let logs: Vec<serde_json::Value> = serde_json::from_str(body.as_str())
            .map_err(|e| format!("Could not parse data from Github: {}\n\n{}", e, body))?;

        let mut new_logs = 0;
        for log in logs {
            // We parsed from JSON so serialization back should be safe
            let log_str = serde_json::to_string(&log).unwrap();
            if !self.canonicalization_contents.contains(&log_str) {
                let timestamp = match log.get("@timestamp") {
                    Some(v) => {
                        if let Some(v) = v.as_u64() {
                            v
                        } else {
                            error!("Got a log from Github without numerical @timestamp field");
                            continue;
                        }
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
                self.logger
                    .send(Message::new(
                        format!("github"),
                        log.0.serialized_log.into_bytes(),
                        LogSource::Generator(Generator::Github),
                        self.config.logbacks_allowed.clone(),
                    ))
                    .unwrap();
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
