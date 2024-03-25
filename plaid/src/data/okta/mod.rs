use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};

use crossbeam_channel::Sender;

use reqwest::Client;

use serde::{Deserialize, Serialize};

use serde_json::Value;

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::executor::Message;

#[derive(Deserialize)]
pub struct OktaConfig {
    token: String,
    domain: String,
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

pub struct Okta {
    client: Client,
    config: OktaConfig,
    since: DateTime<Utc>,
    logger: Sender<Message>,
}

/// We try not to parse anything complicated since our job is just
/// to pass it on.
#[derive(Deserialize, Serialize)]
struct OktaLog {
    published: String,
    actor: Value,
    client: Value,
    device: Value,
    #[serde(rename = "authenticationContext")]
    authentication_context: Value,
    #[serde(rename = "displayMessage")]
    display_message: Value,
    #[serde(rename = "eventType")]
    event_type: Value,
    outcome: Value,
    #[serde(rename = "securityContext")]
    security_context: Value,
    severity: Value,
    #[serde(rename = "debugContext")]
    debug_context: Value,
    #[serde(rename = "legacyEventType")]
    legacy_event_type: Value,
    transaction: Value,
    uuid: Value,
    version: Value,
    request: Value,
    target: Value,
}

impl Okta {
    pub fn new(config: OktaConfig, logger: Sender<Message>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let date = Utc::now();

        Self {
            client,
            config,
            since: date,
            logger,
        }
    }

    pub async fn fetch_system_logs(&mut self) -> Result<(), ()> {
        // Start with the most recent logs
        let mut newest_timestamp = None;
        let mut oldest_timestamp = None;

        loop {
            let address = format!(
                "https://{}/api/v1/logs?sortOrder=DESCENDING&since={:?}",
                self.config.domain, self.since
            );
            let response = self
                .client
                .get(address)
                .header("Accept", "application/json")
                .header("Authorization", format!("SSWS {}", self.config.token))
                .send()
                .await
                .map_err(|e| {
                    println!("Could not get logs from Okta: {}", e);
                })?;

            let body = response
                .text()
                .await
                .map_err(|e| println!("Could not get logs from Okta: {}", e))?;
            let logs: Vec<OktaLog> = serde_json::from_str(body.as_str())
                .map_err(|e| println!("Could not parse data from Okta: {}\n\n{}", e, body))?;

            if logs.is_empty() {
                info!("Okta returned no logs");
                return Ok(());
            }

            let mut counter = 0;
            for log in &logs {
                let log_timestamp = match DateTime::parse_from_rfc3339(&log.published) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(_) => {
                        error!("Got an invalid date from Okta: {}", log.published);
                        continue;
                    }
                };

                if newest_timestamp.is_none() || log_timestamp > newest_timestamp.unwrap() {
                    newest_timestamp = Some(log_timestamp);
                }

                if oldest_timestamp.is_none() || log_timestamp < oldest_timestamp.unwrap() {
                    oldest_timestamp = Some(log_timestamp);
                }

                if log_timestamp < self.since {
                    self.since = newest_timestamp.unwrap();
                    return Ok(());
                }

                counter += 1;
                if log_timestamp > self.since {
                    // Eventually these errors need to bubble up so the service can shut down
                    // then be restarted by an orchestration service
                    self.logger
                        .send(Message::new(
                            format!("okta"),
                            serde_json::to_string(&log).unwrap().as_bytes().to_vec(),
                            LogSource::Generator(Generator::Okta),
                            self.config.logbacks_allowed.clone(),
                        ))
                        .unwrap();
                }
            }
            info!(
                "Sent {} logs for processing. Newest time seen is: {:?}",
                counter, newest_timestamp
            );
            self.since = newest_timestamp.unwrap() + chrono::Duration::milliseconds(1);
        }
    }
}
