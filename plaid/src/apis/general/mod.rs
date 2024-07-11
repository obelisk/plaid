mod logback;
mod network;
mod random;

use crossbeam_channel::Sender;
use reqwest::Client;

use ring::rand::SystemRandom;
use serde::Deserialize;

use std::time::Duration;

use crate::{data::DelayedMessage, executor::Message};

use super::DEFAULT_TIMEOUT_SECONDS;

#[derive(Deserialize)]
pub struct GeneralConfig {
    network: network::Config,
    /// The number of seconds until an external API request times out.
    /// If `None`, the `DEFAULT_TIMEOUT_SECONDS` will be used.
    api_timeout_seconds: Option<u64>,
}

pub struct General {
    config: GeneralConfig,
    client: Client,
    log_sender: Sender<Message>,
    delayed_log_sender: Sender<DelayedMessage>,
    system_random: SystemRandom,
}

impl General {
    pub fn new(
        config: GeneralConfig,
        log_sender: Sender<Message>,
        delayed_log_sender: Sender<DelayedMessage>,
    ) -> Self {
        let timeout_seconds = config
            .api_timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .unwrap();

        let system_random = SystemRandom::new();

        Self {
            config,
            client,
            log_sender,
            delayed_log_sender,
            system_random,
        }
    }
}
