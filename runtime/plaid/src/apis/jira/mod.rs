mod errors;

use std::{sync::Arc, time::Duration};

use plaid_stl::jira::{
    CreateIssueRequest, CreateIssueResponse, GetIssueRequest, GetIssueResponse, GetUserRequest,
    GetUserResponse, PostCommentRequest,
};
use reqwest::Client;
use serde::Deserialize;

use crate::{apis::ApiError, loader::PlaidModule};

use super::default_timeout_seconds;

pub use errors::JiraError;

/// Defines methods to authenticate to Jira with
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum JiraAuthentication {
    Token { token: String },
    // OAuth might be supported in the future
}

impl JiraAuthentication {
    fn to_authorization_header(&self) -> String {
        match self {
            Self::Token { token } => format!("Basic {token}"),
        }
    }
}

#[derive(Deserialize)]
pub struct JiraConfig {
    /// How to authenticate to the Jira API
    authentication: JiraAuthentication,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
    /// The base URL for the Jira API
    base_url: String,
}

/// A representation of the Plaid Jira API
pub struct Jira {
    authentication: JiraAuthentication,
    base_url: String,
    client: Client,
}

impl Jira {
    pub fn new(config: JiraConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        // Remove trailing '/' characters because we will add it later when
        // building the URLs to call
        let base_url = config.base_url.trim_end_matches("/").to_string();

        Self {
            authentication: config.authentication,
            base_url,
            client,
        }
    }

    /// Create a new Jira issue
    pub async fn create_issue(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<CreateIssueRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // TODO Validate the request

        let url = format!("{}/issue", self.base_url);

        // Build the payload
        let payload = request.to_payload();

        info!("Creating a Jira issue on behalf of [{module}]");

        // Make the call
        match self
            .client
            .post(url)
            .header(
                "Authorization",
                self.authentication.to_authorization_header(),
            )
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status() != 201 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    error!("Jira returned {}: {}", status, text);
                    return Err(ApiError::JiraError(JiraError::UnexpectedStatusCode(
                        status.as_u16(),
                    )));
                }

                let body: CreateIssueResponse = resp
                    .json()
                    .await
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))?;

                serde_json::to_string(&body)
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))
            }
            Err(e) => {
                return Err(ApiError::JiraError(JiraError::NetworkError(e)));
            }
        }
    }

    /// Get a Jira issue
    pub async fn get_issue(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetIssueRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // TODO Validate the request

        let url = format!("{}/issue/{}", self.base_url, request.id);

        info!(
            "Getting Jira issue [{}] on behalf of [{module}]",
            request.id
        );

        // Make the call
        match self
            .client
            .get(url)
            .header(
                "Authorization",
                self.authentication.to_authorization_header(),
            )
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status() != 200 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    error!("Jira returned {}: {}", status, text);
                    return Err(ApiError::JiraError(JiraError::UnexpectedStatusCode(
                        status.as_u16(),
                    )));
                }

                let body: GetIssueResponse = resp
                    .json()
                    .await
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))?;

                serde_json::to_string(&body)
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))
            }
            Err(e) => {
                return Err(ApiError::JiraError(JiraError::NetworkError(e)));
            }
        }
    }

    /// Get a Jira user
    pub async fn get_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetUserRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // TODO Validate the request

        let url = format!("{}/user/search?query={}", self.base_url, request.email);

        // Make the call
        info!("Fetching a Jira user account ID on behalf of [{module}]");

        match self
            .client
            .get(url)
            .header(
                "Authorization",
                self.authentication.to_authorization_header(),
            )
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status() != 200 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    error!("Jira returned {}: {}", status, text);
                    return Err(ApiError::JiraError(JiraError::UnexpectedStatusCode(
                        status.as_u16(),
                    )));
                }

                // Internal struct used to deserialize the response from the REST API
                #[derive(Deserialize)]
                struct JiraUser {
                    #[serde(rename = "accountId")]
                    account_id: String,
                    #[serde(rename = "displayName")]
                    display_name: String,
                }

                let users: Vec<JiraUser> = resp
                    .json()
                    .await
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))?;

                let account_id = users
                    .get(0)
                    .ok_or(ApiError::JiraError(JiraError::InvalidResponse))?
                    .account_id
                    .clone();

                let display_name = users.get(0).map(|u| u.display_name.clone());

                let res = GetUserResponse {
                    id: account_id,
                    display_name,
                };

                serde_json::to_string(&res)
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))
            }
            Err(e) => {
                return Err(ApiError::JiraError(JiraError::NetworkError(e)));
            }
        }
    }

    /// Post a comment to a Jira issue
    pub async fn post_comment(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request =
            serde_json::from_str::<PostCommentRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // TODO Validate the request

        let url = format!("{}/issue/{}/comment", self.base_url, request.issue_id);

        // Build the payload and make the call
        let payload = request.to_payload();

        info!(
            "Posting a comment to Jira issue [{}] on behalf of [{module}]",
            request.issue_id
        );

        match self
            .client
            .post(url)
            .header(
                "Authorization",
                self.authentication.to_authorization_header(),
            )
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status() != 201 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    error!("Jira returned {}: {}", status, text);
                    return Err(ApiError::JiraError(JiraError::UnexpectedStatusCode(
                        status.as_u16(),
                    )));
                }

                Ok(0)
            }
            Err(e) => {
                return Err(ApiError::JiraError(JiraError::NetworkError(e)));
            }
        }
    }
}
