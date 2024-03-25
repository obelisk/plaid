use reqwest::Client;

use serde::{Deserialize, Serialize};

use std::time::Duration;

use std::collections::HashMap;

use super::ApiError;

#[derive(Deserialize)]
pub struct SplunkConfig {
    /// The is endpoint to which logs should be sent
    endpoint: String,

    /// This contains a mapping of HEC bearer tokens to service
    /// names
    hec_tokens: HashMap<String, String>,
}

pub struct Splunk {
    config: SplunkConfig,
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
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self { config, client }
    }

    /// Make a post to a preconfigured slack webhook. This should be preferred
    /// over the arbitrary API call
    pub async fn post_hec(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<String, String> = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let hec_name = request.get("hec_name").ok_or(ApiError::MissingParameter("hec_name".to_string()))?.to_string();
        let log = request.get("log").ok_or(ApiError::MissingParameter("log".to_string()))?.to_string();
        let token = self.config.hec_tokens.get(&hec_name).ok_or(ApiError::SplunkError(SplunkError::UnknownHec(hec_name.clone())))?;

        let event: serde_json::Value = serde_json::from_str(&log).map_err(|_| ApiError::BadRequest)?;
        
        let splunk_log = SplunkLog{event};

        let body = serde_json::to_string(&splunk_log).map_err(|_| ApiError::BadRequest)?;

        info!("Sending a message to a log to Splunk HEC: {hec_name} on behalf of: {module}");

        match self
            .client
            .post(self.config.endpoint.clone())
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Authorization", format!("Splunk {token}"))
            .body(body)
            .send().await {
            Ok(r) => {
                let status = r.status();
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::SplunkError(SplunkError::UnexpectedStatusCode(status.as_u16())))
                }
            },
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}
