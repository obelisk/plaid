mod groups;
mod users;

use reqwest::Client;

use serde::Deserialize;

use std::{time::Duration, string::FromUtf8Error};

#[derive(Deserialize)]
pub struct OktaConfig {
    /// The Okta domain to run queries against
    pub domain: String,
    /// The permissioned API key to get user information
    pub token: String,
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
            .timeout(Duration::from_secs(5))
            .build().unwrap();

        Self {
            config,
            client,
        }
    }
}
