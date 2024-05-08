mod logback;
mod network;
mod random;

use crossbeam_channel::Sender;
use reqwest::Client;

use ring::rand::SystemRandom;
use serde::Deserialize;

use std::time::Duration;

use crate::{data::DelayedMessage, executor::Message};

#[derive(Deserialize)]
pub struct GeneralConfig {
    network: network::Config,
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
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
