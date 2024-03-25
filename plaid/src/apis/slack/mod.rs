mod api;
mod webhook;

use reqwest::Client;

use serde::Deserialize;

use std::time::Duration;

use std::collections::HashMap;

#[derive(Deserialize)]
pub struct SlackConfig {
    /// This contains the mapping of preconfigured webhooks modules
    /// are permitted to use
    webhooks: HashMap<String, String>,

    /// This contains the mapping of preconfigured bot tokens that can
    /// be used in various Slack API calls
    bot_tokens: HashMap<String, String>,
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self { config, client }
    }
}
