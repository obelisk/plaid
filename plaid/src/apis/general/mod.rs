mod logback;
mod network;

use crossbeam_channel::Sender;
use reqwest::Client;

use serde::Deserialize;

use std::time::Duration;

use crate::{executor::Message, data::DelayedMessage};

#[derive(Deserialize)]
pub struct GeneralConfig {
    network: network::Config,
}

pub struct General {
    config: GeneralConfig,
    client: Client,
    log_sender: Sender<Message>,
    delayed_log_sender: Sender<DelayedMessage>,
}

impl General {
    pub fn new(config: GeneralConfig, log_sender: Sender<Message>, delayed_log_sender: Sender<DelayedMessage>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build().unwrap();

        Self {
            config,
            client,
            log_sender,
            delayed_log_sender,
        }
    }
}
