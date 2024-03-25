use reqwest::Client;
use serde::{Deserialize};

use std::{collections::HashMap, time::Duration};

mod trigger;

#[derive(Deserialize)]
pub struct PagerDutyConfig {
    /// This is a mapping from service name (that is visible to the service) and to
    /// the integration key relevant to creating an incident in PagerDuty under that
    /// same service
    services: HashMap<String, String>,
}


pub struct PagerDuty {
    config: PagerDutyConfig,
    client: Client,
}

#[derive(Debug)]
pub enum PagerDutyError {
    NetworkError(reqwest::Error)
}

impl PagerDuty {
    pub fn new(config: PagerDutyConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build().unwrap();

        Self {
            config,
            client,
        }
    }
}
