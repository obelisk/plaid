use reqwest::Client;
use serde::Deserialize;

use std::{collections::HashMap, time::Duration};

use super::DEFAULT_TIMEOUT_SECONDS;

mod trigger;

#[derive(Deserialize)]
pub struct PagerDutyConfig {
    /// This is a mapping from service name (that is visible to the service) and to
    /// the integration key relevant to creating an incident in PagerDuty under that
    /// same service
    services: HashMap<String, String>,
    /// The number of seconds until an external API request times out.
    /// If `None`, the `DEFAULT_TIMEOUT_SECONDS` will be used.
    api_timeout_seconds: Option<u64>,
}

pub struct PagerDuty {
    config: PagerDutyConfig,
    client: Client,
}

#[derive(Debug)]
pub enum PagerDutyError {
    NetworkError(reqwest::Error),
}

impl PagerDuty {
    pub fn new(config: PagerDutyConfig) -> Self {
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
