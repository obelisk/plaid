use reqwest::Client;
use serde::Deserialize;

use std::{collections::HashMap, time::Duration};

use super::default_timeout_seconds;

mod trigger;

#[derive(Deserialize)]
pub struct PagerDutyConfig {
    /// This is a mapping from service name (that is visible to the service) and to
    /// the integration key relevant to creating an incident in PagerDuty under that
    /// same service
    services: HashMap<String, String>,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

/// Object to interact with the PagerDuty API
pub struct PagerDuty {
    /// Config for the PagerDuty API
    config: PagerDutyConfig,
    /// Client to make requests with
    client: Client,
}

#[derive(Debug)]
pub enum PagerDutyError {
    NetworkError(reqwest::Error),
}

impl PagerDuty {
    pub fn new(config: PagerDutyConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        Self { config, client }
    }
}
