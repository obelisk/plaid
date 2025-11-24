mod errors;

use std::{collections::HashMap, sync::Arc, time::Duration};

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
    authentication: JiraAuthentication,
    base_url: String,
    client: Client,
    module_permissions: HashMap<String, Vec<String>>,
}

/// Return whether a string is a valid email address
fn is_valid_email(email: &str) -> bool {
    let email_regex = regex::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").unwrap();
    email_regex.is_match(email)
}

/// Return whether a string is a valid Jira issue ID (e.g., ABC-123)
fn is_valid_issue_id(s: &str) -> bool {
    // Up to 10 letters, one dash, up to 10 digits
    let re = regex::Regex::new(r"^[A-Za-z]{1,10}-\d{1,10}$").unwrap();
    re.is_match(s)
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
            module_permissions: config.module_permissions,
        }
    }

    /// Validate that a module is allowed to interact with a Jira project
    fn validate_module_permission(&self, module: &str, project: &str) -> Result<(), ApiError> {
        let res = match self.module_permissions.get(module) {
            Some(v) => v.contains(&project.to_string()),
            _ => false,
        };
        if !res {
            error!("Module [{module}] tried to access Jira project [{project}], but doesn't have permission to");
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
        let issue_id = params.to_string();

        // Validate the request: verify the issue ID is in the form ABC...-1234..., get the project key and ensure the module can access it
        if !is_valid_issue_id(&issue_id) {
            return Err(ApiError::BadRequest);
        }
        // We are sure we can extract a project key because the string has passed validation
        let project = issue_id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

        let url = format!("{}/issue/{}", self.base_url, issue_id);

        info!("Getting Jira issue [{issue_id}] on behalf of [{module}]");

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

    /// Update a Jira issue
    pub async fn update_issue(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request =
            serde_json::from_str::<UpdateIssueRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // Validate the request: verify the issue ID is in the form ABC...-1234..., get the project key and ensure the module can access it
        if !is_valid_issue_id(&request.id) {
            return Err(ApiError::BadRequest);
        }
        // We are sure we can extract a project key because the string has passed validation
        let project = request.id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

        let url = format!("{}/issue/{}", self.base_url, request.id);

        // Build the payload
        let payload = request
            .to_payload()
            .inspect_err(|e| {
                error!("Module [{}] sent an invalid payload: {e}", module.name);
            })
            .map_err(|_| ApiError::BadRequest)?;

        info!(
            "Updating Jira issue [{}] on behalf of [{module}]",
            request.id
        );

        // Make the call
        match self
            .client
            .put(url)
            .header(
                "Authorization",
                self.authentication.to_authorization_header(),
            )
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                // Here technically we should always get a 204 because we do not pass the query parameter
                // `returnIssue=true`. However, it would seem odd to error on a 200, so we accept that as well.
                // If for some reason we do get a 200, we simply ignore the response.
                if resp.status() != 200 && resp.status() != 204 {
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

    /// Get a Jira user
    pub async fn get_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let email = params.to_string();

        if !is_valid_email(&email) {
            return Err(ApiError::BadRequest);
        }

        let url = format!("{}/user/search?query={}", self.base_url, email);

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
        if !is_valid_issue_id(&request.issue_id) {
            return Err(ApiError::BadRequest);
        }
        // We are sure we can extract a project key because the string has passed validation
        let project = request.issue_id.split("-").collect::<Vec<&str>>()[0];

        self.validate_module_permission(&module.name, project)?;

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
