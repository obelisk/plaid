mod api;
mod webhook;

use reqwest::Client;

use serde::Deserialize;

use std::time::Duration;

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
    config: SlackConfig,
    client: Client,
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

        Self { config, client }
    }
}
