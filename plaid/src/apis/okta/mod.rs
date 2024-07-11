mod groups;
mod users;

use reqwest::Client;

use serde::Deserialize;

use std::{string::FromUtf8Error, time::Duration};

use super::DEFAULT_TIMEOUT_SECONDS;

#[derive(Deserialize)]
pub struct OktaConfig {
    /// The Okta domain to run queries against
    pub domain: String,
    /// The permissioned API key to get user information
    pub token: String,
    api_timeout_seconds: Option<u64>,
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
