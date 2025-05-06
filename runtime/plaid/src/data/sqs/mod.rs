use std::num::NonZeroUsize;

use aws_sdk_sqs::Client;
use crossbeam_channel::Sender;
use lru::LruCache;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use serde::Deserialize;

use crate::{executor::Message, get_aws_sdk_config, AwsAuthentication};

#[derive(Deserialize)]
pub struct SQSConfig {
    /// Name of this SQS queue
    pub name: String,
    /// Polling URL for this SQS queue
    pub queue_url: String,
    /// How to authenticate to AWS
    pub authentication: AwsAuthentication,
    /// Denotes if logs produced by this generator are allowed to initiate log backs
    #[serde(default)]
    logbacks_allowed: LogbacksAllowed,
    /// Number of milliseconds to wait in between calls to the API.
    /// If no value is provided here, we will use a default value (10 second).
    #[serde(default = "default_sleep_milliseconds")]
    pub sleep_duration: u64,
}

/// This function provides the default sleep duration in milliseconds.
/// It is used as the default value for deserialization of the `sleep_duration` field,
/// of `SQSConfig` in the event that no value is provided.
fn default_sleep_milliseconds() -> u64 {
    10000
}

/// Represents the entire SQS data generator set up
pub struct SQS {
    /// The configuration of the generator
    pub config: SQSConfig,
    /// API client
    client: Client,
    /// Timestamp of the last seen log we have processed
    /// The logger used to send logs to the execution system for processing
    logger: Sender<Message>,
    /// SQS sends messages 'atleast once' so we use this cache to dedup messages
    /// An LRU where we store the UUIDs of messages that we have already seen and sent into the logging system.
    /// This LRU has a limited capacity: when this is reached, the least-recently-used item is removed to make space for a new insertion.
    /// Note: we only use the "key" part to keep track of the UUIDs we have seen. The "value" part is not used and always set to 0u32.
    seen_messages: LruCache<String, u32>,
}

impl SQS {
    pub async fn new(config: SQSConfig, logger: Sender<Message>) -> Self {
        let sdk_config = get_aws_sdk_config(&config.authentication).await;
        let client = aws_sdk_sqs::Client::new(&sdk_config);

        Self {
            config,
            client,
            seen_messages: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            logger,
        }
    }

    pub async fn drain_queue(&mut self) -> Result<(), String> {
        debug!("sqs/{} draining queue", self.config.name);

        loop {
            // poll the SQS queue
            let res = self
                .client
                .receive_message()
                .queue_url(&self.config.queue_url)
                .max_number_of_messages(10) // just get max if available
                .wait_time_seconds(1) // no long polling
                .send()
                .await
                .map_err(|e| {
                    format!(
                        "sqs/{} receive_messages failed. error: [{e}]",
                        self.config.name
                    )
                })?;

            match res.messages {
                None => {
                    debug!("sqs/{} no messages found", self.config.name);
                    break Ok(());
                }
                Some(messages) => {
                    debug!(
                        "sqs/{} received {} messages",
                        self.config.name,
                        messages.len()
                    );
                    for message in messages {
                        // dedup messages
                        if let Some(id) = message.message_id() {
                            if self.seen_messages.contains(id) {
                                debug!("sqs/{} detected duplicate message {id}", self.config.name);
                                self.delete_message(message.receipt_handle).await?;
                                continue;
                            } else {
                                self.seen_messages.put(id.to_string(), 0u32);
                            }
                        }
                        // consume this message
                        if let Some(body) = message.body {
                            // send event to rules
                            self.send_for_processing(body.as_bytes().to_vec())?;
                            // delete the message from the queue to prevent re-processing
                            self.delete_message(message.receipt_handle).await?;
                        }
                    }
                }
            }
        }
    }

    async fn delete_message(&self, receipt_handle: Option<String>) -> Result<(), String> {
        if let Some(receipt_handle) = receipt_handle {
            let _ = self
                .client
                .delete_message()
                .queue_url(&self.config.queue_url)
                .receipt_handle(receipt_handle)
                .send()
                .await
                .map_err(|e| format!("sqs/{} delete_message failed: [{e}]", self.config.name))?;

            debug!("sqs/{} deleted_message", self.config.name,);
        }
        Ok(())
    }

    fn send_for_processing(&self, payload: Vec<u8>) -> Result<(), String> {
        self.logger
            .send(Message::new(
                format!("sqs/{}", self.config.name),
                payload,
                LogSource::Generator(Generator::SQS(self.config.name.clone())),
                self.config.logbacks_allowed.clone(),
            ))
            .map_err(|e| {
                format!(
                    "sqs/{} send_for_processing failed. error: {e}",
                    self.config.name
                )
            })
    }
}
