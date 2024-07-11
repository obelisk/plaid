mod groups;
mod users;

use reqwest::Client;

use serde::Deserialize;

use std::{string::FromUtf8Error, time::Duration};

use super::default_timeout_seconds;

#[derive(Deserialize)]
pub struct OktaConfig {
    /// The Okta domain to run queries against
    pub domain: String,
    /// The permissioned API key to get user information
    pub token: String,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

pub struct Okta {
    config: OktaConfig,
    client: Client,
}

#[derive(Debug)]
pub enum OktaError {
    BadData(FromUtf8Error),
    UnexpectedStatusCode(u16),
}

impl Okta {
    pub fn new(config: OktaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        Self { config, client }
    }
}
