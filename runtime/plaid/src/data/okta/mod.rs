use crate::executor::Message;
use crossbeam_channel::Sender;
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const OKTA_LOG_PUBLISHED_FIELD_KEY: &str = "published";

#[derive(Deserialize)]
pub struct OktaConfig {
    /// API token used to authenticate to Okta
    token: String,
    /// Domain that API calls will be sent to
    domain: String,
    /// Sets the number of results that are returned in the response
    /// If no value is provided here, we will default to 100.
    #[serde(deserialize_with = "parse_limit")]
    #[serde(default = "default_okta_limit")]
    limit: u16,
    /// Number of milliseconds to wait in between calls to the Okta API.
    /// Okta enforces a rate limit of 50 calls/sec for the `/logs` endpoint.
    /// If no value is provided here, we will default to 1 milliseconds between calls
    #[serde(default = "default_sleep_milliseconds")]
    sleep_duration: u64,
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

/// Custom parser for limit. Returns an error if a limit = 0 or limit > 1000 is given
fn parse_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let limit = u16::deserialize(deserializer)?;

    match limit {
        1..=1000 => Ok(limit),
        0 => Err(serde::de::Error::custom(
            "Invalid limit value provided. Minimum limit is 1",
        )),
        _ => Err(serde::de::Error::custom(
            "Invalid limit value provided. Maximum limit is 1000",
        )),
    }
}

/// This function provides the default sleep duration in milliseconds.
/// It is used as the default value for deserialization of the `sleep_duration` field,
/// of `OktaConfig` in the event that no value is provided.
fn default_sleep_milliseconds() -> u64 {
    1
}

/// This function provides the default limit for the number of system logs returned from Okta.
/// It is used as the default value for deserialization of the `limit` field,
/// of `OktaConfig` in the event that no value is provided.
fn default_okta_limit() -> u16 {
    100
}

pub struct Okta {
    /// A `reqwest` client to send API calls with
    client: Client,
    /// Contains domain and token
    config: OktaConfig,
    /// Filters the lower time bound of the log events published property for bounded queries or persistence time for polling queries
    since: OffsetDateTime,
    /// Sending channel used to send logs into the execution system
    logger: Sender<Message>,
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

        let sleep_duration = Duration::from_millis(self.config.sleep_duration);

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
                "https://{}/api/v1/logs?sortOrder=DESCENDING&since={since}&limit={}",
                self.config.domain, self.config.limit
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
            let logs: Vec<Value> = serde_json::from_str(body.as_str())
                .map_err(|e| error!("Could not parse data from Okta: {e}\n\n{body}"))?;

            // If there have been no new logs since we last polled, we can exit the loop early
            // Exiting here will result in a 10 second wait between restarts
            if logs.is_empty() {
                debug!("No new Okta logs since: {}", self.since);
                return Ok(());
            }

            // Loop over the logs we did get from Okta, attempt to parse their timestamps, and send them into the logging system
            for log in &logs {
                let published = match log
                    .as_object()
                    .and_then(|obj| obj.get(OKTA_LOG_PUBLISHED_FIELD_KEY))
                    .and_then(|val| val.as_str())
                {
                    Some(published) => published,
                    None => {
                        error!("Missing or invalid 'published' field in Okta log: {log:?}",);
                        continue;
                    }
                };

                let log_timestamp = match OffsetDateTime::parse(published, &Rfc3339) {
                    Ok(dt) => dt,
                    Err(_) => {
                        error!("Got an invalid date from Okta: {}", published);
                        continue;
                    }
                };

                // Check if this is the latest log we've seen and update if so
                // We'll use the new most_recent_log_seen time to filter the subsequent
                // API calls to Okta afterwards
                //
                // By default, the Okta API returns the logs in ascending order so we could in theory just
                // take the last timstamp and set it as our max log time. I'm okay with doing another check here in the
                // case that Okta's sorting fails to ensure that we do not miss any logs. The number of comparisions here (1000 max)
                // is nothing to be concerned about.
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

            // Update the time of our most recent log and wait for the specified period
            self.since = most_recent_log_seen + sleep_duration;
            tokio::time::sleep(sleep_duration).await
        }
    }
}
