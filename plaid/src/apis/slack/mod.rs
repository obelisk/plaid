mod api;
mod webhook;

use reqwest::Client;

use serde::Deserialize;

use std::time::Duration;

use std::collections::HashMap;

use super::DEFAULT_TIMEOUT_SECONDS;

#[derive(Deserialize)]
pub struct SlackConfig {
    /// This contains the mapping of preconfigured webhooks modules
    /// are permitted to use
    webhooks: HashMap<String, String>,
    /// This contains the mapping of preconfigured bot tokens that can
    /// be used in various Slack API calls
    bot_tokens: HashMap<String, String>,
    /// The number of seconds until an external API request times out.
    /// If `None`, the `DEFAULT_TIMEOUT_SECONDS` will be used.
    api_timeout_seconds: Option<u64>,
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
}

impl Slack {
    pub fn new(config: SlackConfig) -> Self {
        let timeout_seconds = config
            .api_timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .unwrap();

        Self { config, client }
    }
}
