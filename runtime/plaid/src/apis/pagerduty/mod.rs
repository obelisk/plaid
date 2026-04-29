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
    /// Configuration for the `pagerduty_get_incident_alerts` host call.
    #[serde(default)]
    get_incident_alerts: Option<PagerDutyGetIncidentAlertsConfig>,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

#[derive(Deserialize)]
struct PagerDutyGetIncidentAlertsConfig {
    token: String,
    allowed_rules: Vec<String>,
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
