use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{GithubApiWrapper, ReviewPatAction},
    PlaidFunctionError,
};

#[derive(Deserialize, Serialize)]
pub struct ListFpatRequestsForOrgParams {
    pub org: String,
}

/// Lists approved fine-grained personal access tokens owned by organization members that can access organization resources
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
pub fn list_fpat_requests_for_org(
    client_id: impl Display,
    org: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_fpat_requests_for_org);
    }

    let params = ListFpatRequestsForOrgParams {
        org: org.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_list_fpat_requests_for_org(
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

#[derive(Serialize, Deserialize)]
pub struct ReviewFpatRequestsForOrgParams {
    pub org: String,
    pub pat_request_ids: Vec<u64>,
    pub action: String,
    pub reason: String,
}

/// Approves or denies multiple pending requests to access organization resources via a fine-grained personal access token
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
/// * `pat_request_ids` - Unique identifiers of the requests for access via fine-grained personal access token. Must be formed of between 1 and 100 pat_request_id values
/// * `action` - Action to apply to the requests.
/// * `reason` - Reason for approving or denying the requests. Max 1024 characters.
pub fn review_fpat_requests_for_org(
    client_id: impl Display,
    org: String,
    pat_request_ids: &[u64],
    action: ReviewPatAction,
    reason: String,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, review_fpat_requests_for_org);
    }

    let request = ReviewFpatRequestsForOrgParams {
        org,
        pat_request_ids: pat_request_ids.to_vec(),
        action: match action {
            ReviewPatAction::Approve => "approve".to_string(),
            ReviewPatAction::Deny => "deny".to_string(),
        },
        reason,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params: request,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_review_fpat_requests_for_org(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

#[derive(Deserialize, Serialize)]
pub struct GetReposForFpatParams {
    pub org: String,
    pub request_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_page: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
}

/// Lists the repositories a fine-grained personal access token request is requesting access to
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
/// * `request_id` - Unique identifier of the request for access via fine-grained personal access token.
/// * `per_page` - The number of results per page (max 100)
/// * `page` - The page number of the results to fetch.
pub fn get_repos_for_fpat(
    client_id: impl Display,
    org: impl Display,
    request_id: u64,
    per_page: Option<u64>,
    page: Option<u64>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repos_for_fpat);
    }

    let params = GetReposForFpatParams {
        org: org.to_string(),
        request_id,
        per_page,
        page,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_repos_for_fpat(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    Ok(String::from_utf8(return_buffer).unwrap())
}
