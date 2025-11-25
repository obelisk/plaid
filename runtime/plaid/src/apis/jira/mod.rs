mod errors;
mod validators;

use std::{collections::HashMap, sync::Arc, time::Duration};

use http::{HeaderMap, HeaderValue};
use plaid_stl::jira::{
    CreateIssueRequest, CreateIssueResponse, GetIssueResponse, GetUserResponse, PostCommentRequest,
    UpdateIssueRequest,
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
    /// Mapping between Plaid modules and Jira projects the module is allowed to interact with
    module_permissions: HashMap<String, Vec<String>>,
}

/// A representation of the Plaid Jira API
pub struct Jira {
    base_url: String,
    client: Client,
    validators: HashMap<&'static str, regex::Regex>,
    module_permissions: HashMap<String, Vec<String>>,
}

impl Jira {
    pub fn new(config: JiraConfig) -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&config.authentication.to_authorization_header())
                .map_err(|e| e.to_string())?,
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .default_headers(headers)
            .build()
            .unwrap();

        // Remove trailing '/' characters because we will add it later when
        // building the URLs to call
        let base_url = config.base_url.trim_end_matches("/").to_string();

        let validators = validators::create_validators();

        Ok(Self {
            base_url,
            client,
            validators,
            module_permissions: config.module_permissions,
        })
    }

    /// Validate that a module is allowed to interact with a Jira project
    fn validate_module_permission(&self, module: &str, project: &str) -> Result<(), ApiError> {
        let res = match self.module_permissions.get(module) {
            Some(v) => v.contains(&project.to_string()),
            _ => false,
        };
        if !res {
            warn!("Module [{module}] tried to access Jira project [{project}], but doesn't have permission to");
            return Err(ApiError::BadRequest);
        }
        Ok(())
    }

    /// Create a new Jira issue
    pub async fn create_issue(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<CreateIssueRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // Validate the request: ensure the calling module has permission to interact with the requested Jira project
        self.validate_module_permission(&module.name, &request.project_key)?;

        let url = format!("{}/issue", self.base_url);

        // Build the payload
        let payload = request.to_payload();

        info!(
            "Creating a Jira issue in project [{}] on behalf of [{module}]",
            request.project_key
        );

        // Make the call
        match self.client.post(url).json(&payload).send().await {
            Ok(resp) => {
                if resp.status() != 201 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    warn!("When creating issue in project [{}] on behalf of [{module}], Jira returned {status}: {text}", request.project_key);
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
        let issue_id = params.to_string();

        // Validate the request: verify the issue ID is in the form ABC...-1234..., get the project key and ensure the module can access it
        let issue_id = self.validate_issue_id(&issue_id)?;

        // We are sure we can extract a project key because the string has passed validation
        let project = issue_id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

        let url = format!("{}/issue/{issue_id}", self.base_url);

        info!("Getting Jira issue [{issue_id}] on behalf of [{module}]");

        // Make the call
        match self.client.get(url).send().await {
            Ok(resp) => {
                if resp.status() != 200 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    warn!("Jira returned {status}: {text}");
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

    /// Update a Jira issue
    pub async fn update_issue(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request =
            serde_json::from_str::<UpdateIssueRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // Validate the request: verify the issue ID is in the form ABC...-1234..., get the project key and ensure the module can access it
        let issue_id = self.validate_issue_id(&request.id)?;

        // We are sure we can extract a project key because the string has passed validation
        let project = issue_id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

        let url = format!("{}/issue/{issue_id}", self.base_url);

        // Build the payload
        let payload = request
            .to_payload()
            .inspect_err(|e| {
                warn!("Module [{}] sent an invalid payload: {e}", module.name);
            })
            .map_err(|_| ApiError::BadRequest)?;

        info!("Updating Jira issue [{issue_id}] on behalf of [{module}]");

        // Make the call
        match self.client.put(url).json(&payload).send().await {
            Ok(resp) => {
                // Here technically we should always get a 204 because we do not pass the query parameter
                // `returnIssue=true`. However, it would seem odd to error on a 200, so we accept that as well.
                // If for some reason we do get a 200, we simply ignore the response.
                if resp.status() != 200 && resp.status() != 204 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    warn!("Jira returned {status}: {text}");
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

    /// Get a Jira user
    pub async fn get_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let email = params.to_string();
        let email = self.validate_email(&email)?;

        let url = format!("{}/user/search?query={email}", self.base_url);

        // Make the call
        info!("Fetching a Jira user account ID on behalf of [{module}]");

        match self.client.get(url).send().await {
            Ok(resp) => {
                if resp.status() != 200 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    warn!("Jira returned {status}: {text}");
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
                    display_name: Option<String>,
                }

                let users: Vec<JiraUser> = resp
                    .json()
                    .await
                    .map_err(|_| ApiError::JiraError(JiraError::InvalidResponse))?;

                // A search-by-email should return a single user: we take the first item in the vec
                let res = users
                    .get(0)
                    .map(|u| GetUserResponse {
                        id: u.account_id.clone(),
                        display_name: u.display_name.clone(),
                    })
                    .ok_or(ApiError::JiraError(JiraError::InvalidResponse))?;

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

        // Validate the request: verify the issue ID is in the form ABC...-1234..., get the project key and ensure the module can access it
        let issue_id = self.validate_issue_id(&request.issue_id)?;

        // We are sure we can extract a project key because the string has passed validation
        let project = issue_id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

        let url = format!("{}/issue/{issue_id}/comment", self.base_url);

        // Build the payload and make the call
        let payload = request.to_payload();

        info!("Posting a comment to Jira issue [{issue_id}] on behalf of [{module}]");

        match self.client.post(url).json(&payload).send().await {
            Ok(resp) => {
                if resp.status() != 201 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_else(|_| "N/A".to_string());
                    warn!("Jira returned {status}: {text}");
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
