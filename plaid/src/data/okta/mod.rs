use crate::executor::Message;
use crossbeam_channel::Sender;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Deserialize)]
pub struct OktaConfig {
    /// API token used to authenticate to Okta
    token: String,
    /// Domain that API calls will be sent to
    domain: String,
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

pub struct Okta {
    /// A `reqwest` client to send API calls with
    client: Client,
    /// Contains domain and token
    config: OktaConfig,
    /// The most recent time we have polled Okta for new system logs. This value is used
    /// to filter the response from the Okta API.
    since: OffsetDateTime,
    /// Sending channel used to send logs into the execution system
    logger: Sender<Message>,
}

/// We try not to parse anything complicated since our job is just
/// to pass it on.
/// See https://developer.okta.com/docs/reference/api/system-log/#logevent-object for full docs
#[derive(Deserialize, Serialize)]
struct OktaLog {
    /// Timestamp when the event is published
    published: String,
    /// Describes the entity that performs an action
    actor: Value,
    /// The client that requests an action
    client: Value,
    /// Type of device that the client operates from (for example, Computer)
    device: Value,
    /// The authentication data of an action
    #[serde(rename = "authenticationContext")]
    authentication_context: Value,
    /// The display message for an event
    #[serde(rename = "displayMessage")]
    display_message: Value,
    /// Type of event that is published
    #[serde(rename = "eventType")]
    event_type: Value,
    /// The outcome of an action
    outcome: Value,
    /// The security data of an action
    #[serde(rename = "securityContext")]
    security_context: Value,
    /// Indicates how severe the event is: DEBUG, INFO, WARN, ERROR
    severity: Value,
    /// The debug request data of an action
    #[serde(rename = "debugContext")]
    debug_context: Value,
    /// Associated Events API Action
    #[serde(rename = "legacyEventType")]
    legacy_event_type: Value,
    /// The transaction details of an action
    transaction: Value,
    /// Unique identifier for an individual event
    uuid: Value,
    /// Versioning indicator
    version: Value,
    /// The request that initiates an action
    request: Value,
    /// Zero or more targets of an action
    target: Value,
}

impl Okta {
    pub fn new(config: OktaConfig, logger: Sender<Message>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self {
            client,
            config,
            since: OffsetDateTime::now_utc(),
            logger,
        }
    }

    pub async fn fetch_system_logs(&mut self) -> Result<(), ()> {
        // Start with the most recent logs
        let mut most_recent_log_seen = OffsetDateTime::UNIX_EPOCH;

        loop {
            // Okta requires the query parameter to be in RFC3339 format. We attempt to format it here.
            // On failure, we return and allow the data orchestrator to restart the loop
            let since = match self.since.format(&Rfc3339) {
                Ok(since) => since,
                Err(e) => {
                    error!("Failed to parse datetime to RFC3339 format. Error: {e}");
                    return Ok(());
                }
            };

            let address = format!(
                "https://{}/api/v1/logs?sortOrder=DESCENDING&since={since}",
                self.config.domain,
            );

            let response = self
                .client
                .get(address)
                .header("Accept", "application/json")
                .header("Authorization", format!("SSWS {}", self.config.token))
                .send()
                .await
                .map_err(|e| {
                    error!("Could not get logs from Okta: {e}");
                })?;

            // Check the response status code
            // If it's outside of the 2XX range, we log the error and exit the loop, allowing the
            // data generator to handle a restart
            if !response.status().is_success() {
                let status = response.status();
                let error_body = response.text().await.ok();
                error!(
                    "Call to Okta API failed with code: {status}. Error: {}",
                    error_body.unwrap_or_default()
                );
                return Ok(());
            }

            // Get the body from the response from Okta
            let body = response
                .text()
                .await
                .map_err(|e| error!("Could not get logs from Okta: {e}"))?;

            // Attempt to deserialize the response from Okta
            let logs: Vec<OktaLog> = serde_json::from_str(body.as_str())
                .map_err(|e| error!("Could not parse data from Okta: {e}\n\n{body}"))?;

            // If there have been no new logs since we last polled, we can exit the loop early
            // Exiting here will result in a 10 second wait between restarts
            if logs.is_empty() {
                debug!("No new Okta logs since: {}", self.since);
                return Ok(());
            }

            // Loop over the logs we did get from Okta, attempt to parse their timestamps, and send them into the logging system
            for log in &logs {
                let log_timestamp = match OffsetDateTime::parse(&log.published, &Rfc3339) {
                    Ok(dt) => dt,
                    Err(_) => {
                        error!("Got an invalid date from Okta: {}", log.published);
                        continue;
                    }
                };

                // Check if this is the latest log we've seen and update if so
                // We'll use the new most_recent_log_seen time to filter the subsequent
                // API calls to Okta afterwards
                if log_timestamp > most_recent_log_seen {
                    most_recent_log_seen = log_timestamp;
                }

                // Attempts to parse the log received from Okta to bytes.
                let log_bytes = match serde_json::to_vec(&log) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Failed to serialize Okta logs to bytes. Error: {e}");
                        continue;
                    }
                };

                // Send log into logging system to be processed by rule(s)
                //
                // Eventually these errors need to bubble up so the service can shut down
                // then be restarted by an orchestration service
                self.logger
                    .send(Message::new(
                        "okta".to_string(),
                        log_bytes,
                        LogSource::Generator(Generator::Okta),
                        self.config.logbacks_allowed.clone(),
                    ))
                    .unwrap();
            }
            info!(
                "Sent {} Okta logs for processing. Newest time seen is: {most_recent_log_seen}",
                logs.len(),
            );

            // Update the time of our most recent log
            self.since = most_recent_log_seen + Duration::from_millis(1);
        }
    }
}
