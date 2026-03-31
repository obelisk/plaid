//! This module provides a way for Plaid to log to InfluxDB.

use std::{collections::HashMap, time::Duration};

use crate::logging::Log;

use super::{LoggingError, PlaidLogger, WrappedLog};

use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug)]
pub enum InfluxDbError {
    SendError,
    UnexpectedStatusCode(u16),
}

/// Configuration for the InfluxDB logger.
#[derive(Deserialize)]
pub struct Config {
    /// The endpoint of the InfluxDB instance (e.g. "http://localhost").
    pub endpoint: String,
    /// The port of the InfluxDB instance (e.g. 8181).
    pub port: u16,
    /// The authentication token for the InfluxDB instance.
    pub token: String,
    /// The name of the InfluxDB database to write to.
    pub database: String,
    /// The timeout for sending logs to the InfluxDB instance, in seconds.
    pub client_timeout: Option<u8>,
}

/// A logger that sends logs to an InfluxDB instance.
pub struct InfluxDBLogger {
    /// The URL of the InfluxDB instance (e.g. "http://localhost:8181").
    url: String,
    /// The authentication token for the InfluxDB instance.
    token: String,
    /// The name of the InfluxDB database to write to.
    database: String,
    /// The HTTP client used to send requests to the InfluxDB instance.
    client: reqwest::Client,
}

impl InfluxDBLogger {
    pub fn new(config: Config) -> Self {
        let mut timeout = config.client_timeout.unwrap_or(30);
        if timeout == 0 {
            timeout = 30; // Default to 30 seconds if the provided timeout is 0
        }
        // Unwrap is safe here because we are providing a valid timeout value and reqwest's ClientBuilder should not fail with a valid timeout.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout.into()))
            .build()
            .unwrap();

        Self {
            url: format!("{}:{}", config.endpoint, config.port),
            token: config.token,
            database: config.database,
            client,
        }
    }

    /// Send a log to the InfluxDB instance using InfluxDB's line protocol.
    async fn send_line_protocol(
        &self,
        table: &str,
        tags: Option<HashMap<&str, &str>>,
        fields: HashMap<&str, &str>,
        timestamp: Option<u64>,
    ) -> Result<(), InfluxDbError> {
        let line = format!(
            "{},{} {} {}",
            table,
            tags.map(|t| t
                .into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(","))
                .unwrap_or_default(),
            fields
                .into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(","),
            timestamp.map(|t| t.to_string()).unwrap_or_default()
        );

        let response = self
            .client
            .post(format!("{}/api/v3/write_lp?db={}", self.url, self.database))
            .header("Authorization", format!("Bearer {}", self.token))
            .body(line)
            .send()
            .await
            .map_err(|_| InfluxDbError::SendError)?;

        if !response.status().is_success() {
            error!(
                "Failed to send log to InfluxDB. Status: {}",
                response.status()
            );
            return Err(InfluxDbError::UnexpectedStatusCode(
                response.status().as_u16(),
            ));
        }

        Ok(())
    }
}

impl PlaidLogger for InfluxDBLogger {
    async fn send_log(&self, log: &WrappedLog) -> Result<(), LoggingError> {
        match log.log {
            Log::TimeseriesPoint {
                ref measurement,
                value,
            } => self
                .send_line_protocol(
                    &measurement,
                    None,
                    [("value", value.to_string().as_str())].into(),
                    None,
                )
                .await
                .map_err(LoggingError::InfluxDbError),
            _ => {
                // For now, we only support timeseries points for InfluxDB logging
                Ok(())
            }
        }
    }
}
