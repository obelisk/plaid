use super::{default_timeout_seconds, ApiError};
use crate::loader::PlaidModule;
use plaid_stl::splunk::PostLogRequest;
use reqwest::{Client, Error, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::{sync::Arc, time::Duration};

#[derive(Deserialize)]
pub struct SplunkConfig {
    /// The is endpoint to which logs should be sent
    endpoint: String,
    /// This contains a mapping of HEC bearer tokens to service
    /// names
    hec_tokens: HashMap<String, String>,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

pub struct Splunk {
    /// Config for the Splunk API
    config: SplunkConfig,
    /// A client to make requests with
    client: Client,
}

#[derive(Serialize)]
struct SplunkLog {
    event: serde_json::Value,
}

#[derive(Debug)]
pub enum SplunkError {
    UnknownHec(String),
    UnexpectedStatusCode(u16),
}

impl Splunk {
    pub fn new(config: SplunkConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .expect("Failed to build reqwest client for Splunk API");

        Self { config, client }
    }

    /// Make a post to a preconfigured slack webhook. This should be preferred
    /// over the arbitrary API call
    pub async fn post_hec(&self, params: &str, module: Arc<PlaidModule>) -> Result<u32, ApiError> {
        let request: PostLogRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let hec_name = request.hec_name;
        let token = self
            .config
            .hec_tokens
            .get(&hec_name)
            .ok_or(ApiError::SplunkError(SplunkError::UnknownHec(
                hec_name.clone(),
            )))?;

        let event: serde_json::Value =
            serde_json::from_str(&request.data).map_err(|_| ApiError::BadRequest)?;

        let splunk_log = SplunkLog { event };

        let body = serde_json::to_string(&splunk_log).map_err(|_| ApiError::BadRequest)?;

        let future = self
            .client
            .post(self.config.endpoint.clone())
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Authorization", format!("Splunk {token}"))
            .body(body)
            .send();

        if request.blocking {
            info!(
                "Sending log message to Splunk HEC (blocking): {hec_name} on behalf of: {module}"
            );
            self.post_log_blocking(future).await
        } else {
            info!("Sending log message to Splunk HEC (non-blocking): {hec_name} on behalf of: {module}");
            self.post_log_non_blocking(future, module);
            Ok(0)
        }
    }

    /// Executes a blocking HTTP request to Splunk HEC and waits for the response.
    /// Returns an error if the request fails or receives a non-success status code.
    async fn post_log_blocking<F>(&self, future: F) -> Result<u32, ApiError>
    where
        F: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        match future.await {
            Ok(r) => {
                if r.status().is_success() {
                    Ok(0)
                } else {
                    Err(ApiError::SplunkError(SplunkError::UnexpectedStatusCode(
                        r.status().as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    /// Spawns a background task to execute the HTTP request to Splunk HEC.
    /// Errors are logged but do not propagate to the caller.
    fn post_log_non_blocking<F>(&self, future: F, module: Arc<PlaidModule>)
    where
        F: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        let module_name = module.to_string();

        tokio::spawn(async move {
            match future.await {
                Ok(r) => {
                    if !r.status().is_success() {
                        error!(
                            "Non-blocking Splunk HEC log post on behalf of {module_name} failed with status: {}",
                            r.status()
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "Non-blocking Splunk HEC log post on behalf of {module_name} errored: {e}"
                    );
                }
            }
        });
    }
}
