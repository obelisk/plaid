use std::num::NonZeroUsize;

use aws_sdk_sqs::Client;
use crossbeam_channel::Sender;
use lru::LruCache;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{executor::Message, get_aws_sdk_config, AwsAuthentication};

use super::{DataGenerator, DataGeneratorLog};

#[derive(Deserialize)]
pub struct SQSConfig {
    /// Name of this SQS queue
    pub name: String,
    /// Polling URL for this SQS queue
    pub queue_url: String,
    /// Polling interval for this SQS queue
    pub wait_time_seconds: i32,
    /// Max number of messsages to get 1 - 10
    pub max_number_of_messages: i32,
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
/// of `SQSConfig` in the event that no value is provided.
fn default_sleep_milliseconds() -> u64 {
    1000
}

fn default_canon_time() -> u64 {
    // guestimate
    20
}

/// Represents the entire SQS data generator set up
pub struct SQS {
    /// The configuration of the generator
    config: SQSConfig,
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

impl SQS {
    pub async fn new(config: SQSConfig, logger: Sender<Message>) -> Self {
        let sdk_config = get_aws_sdk_config(&config.authentication).await;
        let client = aws_sdk_sqs::Client::new(&sdk_config);

        Self {
            config,
            client,
            last_seen: OffsetDateTime::now_utc(),
            seen_logs_uuid: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            logger,
        }
    }
}

impl DataGenerator for &mut SQS {
    async fn fetch_logs(
        &self,
        _since: time::OffsetDateTime,
        _until: time::OffsetDateTime,
    ) -> Result<Vec<DataGeneratorLog>, ()> {
        let mut logs = vec![];

        // poll the SQS queue
        let res = self
            .client
            .receive_message()
            .queue_url(&self.config.queue_url)
            .max_number_of_messages(self.config.max_number_of_messages)
            .wait_time_seconds(self.config.wait_time_seconds)
            .send()
            .await
            .map_err(|e| {
                error!("SQS receive_messages failed. error: [{e}]");
            })?;

        // Process messages if any
        if let Some(messages) = res.messages {
            for message in messages {
                // Print message body if present
                if let Some(body) = message.body {
                    // parse the payload to extract timestamp
                    let value = serde_json::from_str::<serde_json::Value>(&body)
                        .map_err(|e| error!("failed to decode SQS message body. error: {e}"))?;
                    let timestamp = if let Some(serde_json::Value::String(t)) =
                        value.pointer("/time")
                    {
                        // Parse the timestamp into an OffsetDateTime
                        OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339)
                                .map_err(|e| {
                                    error!("failed to parse OffsetDateTime from SQS message.time: {t} error: {e}")
                                })?
                    } else {
                        // default to the timestamp the message was recieved
                        OffsetDateTime::now_utc()
                    };

                    // send to rules
                    let id = message
                        .message_id
                        .ok_or(error!("SQS message did not have message_id"))?;

                    logs.push(DataGeneratorLog {
                        id,
                        timestamp,
                        payload: body.as_bytes().to_vec(),
                    });

                    // Delete the message from the queue to prevent re-processing
                    if let Some(receipt_handle) = message.receipt_handle {
                        self.client
                            .delete_message()
                            .queue_url(&self.config.queue_url)
                            .receipt_handle(receipt_handle)
                            .send()
                            .await
                            .map_err(|e| {
                                error!("Could not get events from SQS: [{e}]");
                            })?;
                        println!("Deleted message from queue");
                    }
                }
            }
        }

        Ok(logs)
    }

    fn get_name(&self) -> String {
        "SQS".to_string()
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
                format!("sqs/{}", self.config.name),
                payload,
                LogSource::Generator(Generator::SQS(self.config.name.clone())),
                self.config.logbacks_allowed.clone(),
            ))
            .unwrap();
    }
}
