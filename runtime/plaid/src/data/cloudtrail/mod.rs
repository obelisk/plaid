use std::num::NonZeroUsize;

use aws_sdk_cloudtrail::Client;
use aws_sdk_kms::primitives::DateTime;
use crossbeam_channel::Sender;
use lru::LruCache;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{executor::Message, get_aws_sdk_config, AwsAuthentication};

use super::DataGenerator;

#[derive(Deserialize)]
pub struct CloudtrailConfig {
    /// How to authenticate to AWS
    pub authentication: AwsAuthentication,
    /// Denotes if logs produced by this generator are allowed to initiate log backs
    #[serde(default)]
    logbacks_allowed: LogbacksAllowed,
    /// Canonicalization time, i.e., after how many seconds we can consider logs as "stable"
    #[serde(default = "default_canon_time")]
    canon_time: u64,
    /// Number of milliseconds to wait in between calls to the API.
    /// If no value is provided here, we will use a default value (1 second).
    #[serde(default = "default_sleep_milliseconds")]
    sleep_duration: u64,
}

/// This function provides the default sleep duration in milliseconds.
/// It is used as the default value for deserialization of the `sleep_duration` field,
/// of `CloudtrailConfig` in the event that no value is provided.
fn default_sleep_milliseconds() -> u64 {
    1000
}

fn default_canon_time() -> u64 {
    30
}

/// Represents the entire Cloudtrail data generator set up
pub struct Cloudtrail {
    /// The configuration of the generator
    config: CloudtrailConfig,
    /// API client
    client: Client,
    /// Timestamp of the last seen log we have processed
    last_seen: OffsetDateTime,
    /// The logger used to send logs to the execution system for processing
    logger: Sender<Message>,
    /// An LRU where we store the UUIDs of logs that we have already seen and sent into the logging system.
    /// This, together with some overlapping queries to the API, helps us ensure that all logs are processed
    /// exactly once.
    /// This LRU has a limited capacity: when this is reached, the least-recently-used item is removed to make space for a new insertion.
    /// Note: we only use the "key" part to keep track of the UUIDs we have seen. The "value" part is not used and always set to 0u32.
    seen_logs_uuid: LruCache<String, u32>,
}

impl Cloudtrail {
    pub async fn new(config: CloudtrailConfig, logger: Sender<Message>) -> Self {
        let sdk_config = get_aws_sdk_config(&config.authentication).await;
        let client = aws_sdk_cloudtrail::Client::new(&sdk_config);

        Self {
            config,
            client,
            last_seen: OffsetDateTime::now_utc(),
            seen_logs_uuid: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            logger,
        }
    }
}

impl DataGenerator for &mut Cloudtrail {
    async fn fetch_logs(
        &self,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
    ) -> Result<Vec<super::DataGeneratorLog>, ()> {
        let mut next_token: Option<String> = None;
        let mut logs = vec![];

        // Loop through the pages of events
        loop {
            let res = self
                .client
                .lookup_events()
                .start_time(DateTime::from_secs(since.unix_timestamp()))
                .end_time(DateTime::from_secs(until.unix_timestamp()))
                .set_next_token(next_token)
                .send()
                .await
                .map_err(|e| {
                    error!("Could not get events from Cloudtrail: [{e}]");
                })?;
            // If we got some events, then we process them. Otherwise break.
            if let Some(events) = res.events {
                // Process Cloudtrail events and convert them into logs, then collect everything into a vector.
                // We keep only events for which we have an ID, a timestamp and a payload.
                let log_page: Vec<super::DataGeneratorLog> = events
                    .into_iter()
                    .filter_map(|event| {
                        let id = event.event_id;
                        let timestamp = event.event_time.map(|t| {
                            let unix = t.secs();
                            let odt = OffsetDateTime::from_unix_timestamp(unix).unwrap();
                            odt
                        });
                        let payload = event.cloud_trail_event.map(|ev| ev.into_bytes());
                        match (id, timestamp, payload) {
                            (Some(id), Some(timestamp), Some(payload)) => {
                                // We have all the pieces: assemble a DataGeneratorLog
                                let log = super::DataGeneratorLog {
                                    id,
                                    timestamp,
                                    payload,
                                };
                                Some(log)
                            }
                            // Otherwise, skip this event
                            _ => None,
                        }
                    })
                    .collect();
                logs.extend(log_page);

                next_token = res.next_token;
                if next_token.is_none() {
                    // Reached the last page
                    break;
                }
            } else {
                break;
            }
        }
        Ok(logs)
    }

    fn get_name(&self) -> String {
        "Cloudtrail".to_string()
    }

    fn get_sleep_duration(&self) -> u64 {
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

    fn send_for_processing(&self, payload: Vec<u8>) {
        self.logger
            .send(Message::new(
                format!("cloudtrail"),
                payload,
                LogSource::Generator(Generator::Cloudtrail),
                self.config.logbacks_allowed.clone(),
            ))
            .unwrap();
    }
}
