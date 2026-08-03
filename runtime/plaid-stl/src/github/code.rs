use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{github::GithubApiWrapper, PlaidFunctionError};

/// Parameters sent to the runtime when creating a GitHub App installation access token.
/// See https://docs.github.com/en/rest/apps/apps?apiVersion=2026-03-10#create-an-installation-access-token-for-an-app
#[derive(Serialize, Deserialize)]
pub struct CreateInstallationAccessTokenParams {
    pub installation_id: u64,
    #[serde(flatten)]
    pub body: CreateInstallationAccessTokenBody,
}

/// Body sent to the GitHub API when creating an installation access token.
#[derive(Serialize, Deserialize)]
pub struct CreateInstallationAccessTokenBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "repository_ids")]
    pub repository_ids: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
pub struct InstallationAccessTokenCreationResponse {
    pub token: String,
    pub expires_at: String,
}

/// Create an installation access token for a GitHub App installation.
/// See https://docs.github.com/en/rest/apps/apps?apiVersion=2026-03-10#create-an-installation-access-token-for-an-app for more details
/// Note that this endpoint is only available to GitHub Apps, and not to OAuth apps or personal access tokens.
///
/// # Arguments
/// * `client_id` - The client ID of the GitHub App making the request.
/// * `params` - The parameters for creating the installation access token.
pub fn create_installation_access_token(
    client_id: impl Display,
    params: &CreateInstallationAccessTokenParams,
) -> Result<InstallationAccessTokenCreationResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, create_installation_access_token);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapped).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_create_installation_access_token(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let response = String::from_utf8(return_buffer).unwrap();
    Ok(serde_json::from_str(&response).map_err(|_| PlaidFunctionError::InternalApiError)?)
}
