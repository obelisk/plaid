mod api;
mod webhook;

use reqwest::Client;

use serde::Deserialize;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use super::default_timeout_seconds;

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
    /// Per-channel earliest-next-post time. Used to pace `chat.postMessage`
    /// to <=1/sec/channel so alert bursts don't trip Slack's 429s.
    post_pacing: Mutex<HashMap<String, Instant>>,
}

#[derive(Debug)]
pub enum SlackError {
    UnknownHook(String),
    UnknownBot(String),
    UnexpectedStatusCode(u16),
    UnexpectedPayload(String),
}

impl Slack {
    pub fn new(config: SlackConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        Self {
            config,
            client,
            post_pacing: Mutex::new(HashMap::new()),
        }
    }
}
