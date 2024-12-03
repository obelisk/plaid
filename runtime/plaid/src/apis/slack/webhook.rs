use std::collections::HashMap;

use crate::apis::{slack::SlackError, ApiError};

use super::Slack;

impl Slack {
    /// Make a post to a given slack webhook.
    pub async fn post_to_arbitrary_webhook(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let hook = request
            .get("hook")
            .ok_or(ApiError::MissingParameter("hook".to_string()))?
            .to_string();
        let body = request
            .get("body")
            .ok_or(ApiError::MissingParameter("body".to_string()))?
            .to_string();

        let address = format!("https://hooks.slack.com/services/{}", hook);

        match self.client.post(address).body(body).send().await {
            Ok(r) => {
                let status = r.status();
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                        status.as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    /// Make a post to a preconfigured slack webhook. This should be preferred
    /// over the arbitrary API call
    pub async fn post_to_named_webhook(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let hook_name = request
            .get("hook_name")
            .ok_or(ApiError::MissingParameter("hook_name".to_string()))?
            .to_string();
        let body = request
            .get("body")
            .ok_or(ApiError::MissingParameter("body".to_string()))?
            .to_string();

        let hook = self
            .config
            .webhooks
            .get(&hook_name)
            .ok_or(ApiError::SlackError(SlackError::UnknownHook(
                hook_name.clone(),
            )))?;

        info!("Sending a message to a Slack webhook: {hook_name} on behalf of: {module}");
        let address = format!("https://hooks.slack.com/services/{}", hook);

        match self.client.post(address).body(body).send().await {
            Ok(r) => {
                let status = r.status();
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                        status.as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}
