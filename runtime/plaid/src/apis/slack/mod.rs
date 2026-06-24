mod api;
mod queue;
mod webhook;

use reqwest::Client;

use serde::Deserialize;

use std::sync::Arc;
use std::time::Duration;

use std::collections::HashMap;

use tokio::sync::mpsc::UnboundedSender;

use crate::storage::Storage;

use super::default_timeout_seconds;

pub(crate) use queue::QueuedPost;

/// Slack `chat.postMessage` endpoint, used by the outbound drain task.
pub(crate) const SLACK_POST_MESSAGE_URL: &str = "https://slack.com/api/chat.postMessage";

#[derive(Deserialize)]
pub struct SlackConfig {
    /// This contains the mapping of preconfigured webhooks modules
    /// are permitted to use
    webhooks: HashMap<String, String>,
    /// This contains the mapping of preconfigured bot tokens that can
    /// be used in various Slack API calls
    bot_tokens: HashMap<String, String>,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

pub struct Slack {
    /// Config for the Slack API
    config: SlackConfig,
    /// A client to make requests with
    client: Client,
    /// Durable storage backing the outbound queue (None if storage isn't configured).
    storage: Option<Arc<Storage>>,
    /// Sender into the background outbound-queue drain task (None if no storage).
    queue_tx: Option<UnboundedSender<QueuedPost>>,
}

#[derive(Debug)]
pub enum SlackError {
    UnknownHook(String),
    UnknownBot(String),
    UnexpectedStatusCode(u16),
    UnexpectedPayload(String),
}

impl Slack {
    pub fn new(config: SlackConfig, storage: Option<Arc<Storage>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        // Spawn the outbound-queue drain task when durable storage is available.
        // Api::new runs on the main tokio runtime, so tokio::spawn is valid here.
        let queue_tx = storage.as_ref().map(|storage| {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(queue::run(
                rx,
                client.clone(),
                config.bot_tokens.clone(),
                storage.clone(),
            ));
            tx
        });

        Self {
            config,
            client,
            storage,
            queue_tx,
        }
    }
}
