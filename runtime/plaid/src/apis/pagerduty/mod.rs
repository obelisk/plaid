use reqwest::Client;
use serde::Deserialize;

use std::{collections::HashMap, time::Duration};

use super::default_timeout_seconds;

mod incident_alerts;
mod trigger;

#[derive(Deserialize)]
pub struct PagerDutyConfig {
    /// This is a mapping from service name (that is visible to the service) and to
    /// the integration key relevant to creating an incident in PagerDuty under that
    /// same service
    #[serde(default)]
    services: HashMap<String, String>,
    /// REST API configuration for read operations.
    #[serde(default)]
    rest: Option<PagerDutyRestConfig>,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

#[derive(Deserialize)]
struct PagerDutyRestConfig {
    token: String,
    incident_alerts: PagerDutyRestEndpointConfig,
}

#[derive(Deserialize)]
struct PagerDutyRestEndpointConfig {
    allowed_rules: Vec<String>,
    #[serde(default)]
    available_in_test_mode: bool,
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
    UnexpectedStatusCode(u16),
    UnexpectedPayload,
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
