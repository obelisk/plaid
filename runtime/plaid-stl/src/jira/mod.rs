mod utils;

use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

/// Request sent to the runtime to create a Jira issue
#[derive(Serialize, Deserialize)]
pub struct CreateIssueRequest {
    pub project_key: String,
    pub summary: String,
    /// This is a JSON payload in the format expected by Jira
    pub description: Value,
    // The caller might decide to use issuetype.id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuetype_name: Option<String>,
    // We use Value because sometimes Jira expects arrays or other objects
    pub other_fields: HashMap<String, Value>,
}

/// Response received from the runtime when creating a Jira issue
#[derive(Serialize, Deserialize)]
pub struct CreateIssueResponse {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_: String,
}

/// Response received from the runtime when fetching a Jira issue
#[derive(Serialize, Deserialize)]
pub struct GetIssueResponse {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_: String,
    pub fields: Value,
}

/// Request sent to the runtime to update a Jira issue
#[derive(Serialize, Deserialize)]
pub struct UpdateIssueRequest {
    pub id: String,
    /// This is used to overwrite values
    pub fields: Option<Value>,
    /// This is more granular and can be used to update values
    /// (e.g., adding/removing items from arrays)
    pub update: Option<Value>,
}

/// Response received from the runtime when fetching info about a Jira user
#[derive(Serialize, Deserialize)]
pub struct GetUserResponse {
    pub display_name: Option<String>,
    pub id: String,
}

/// Request sent to the runtime to post a comment to a Jira issue
#[derive(Serialize, Deserialize)]
pub struct PostCommentRequest {
    pub issue_id: String,
    pub comment: String,
}

/// Request sent to the runtime to search for Jira issues
#[derive(Serialize, Deserialize)]
pub struct SearchIssueRequest {
    pub jql: String,
    pub max_results: Option<u32>,
}

/// Represents a Jira issue with only the most basic fields (id and key)
#[derive(Serialize, Deserialize)]
pub struct JiraIssue {
    pub id: String,
    pub key: String,
}

/// Response received from the runtime when searching for Jira issues
#[derive(Serialize, Deserialize)]
pub struct SearchIssueResponse {
    pub issues: Vec<JiraIssue>,
}

// ==============================================================================================================

/// Create a Jira issue
pub fn create_issue(
    payload: CreateIssueRequest,
) -> Result<CreateIssueResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, create_issue);
    }

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        jira_create_issue(
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
    Ok(serde_json::from_str(&String::from_utf8(return_buffer).unwrap()).unwrap())
}

/// Fetch a Jira issue
pub fn get_issue(id: impl Display) -> Result<GetIssueResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, get_issue);
    }

    let request = id.to_string();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        jira_get_issue(
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
    Ok(serde_json::from_str(&String::from_utf8(return_buffer).unwrap()).unwrap())
}

/// Update a Jira issue
pub fn update_issue(payload: UpdateIssueRequest) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(jira, update_issue);
    }

    let request = serde_json::to_string(&payload).unwrap();
    let res = unsafe { jira_update_issue(request.as_bytes().as_ptr(), request.as_bytes().len()) };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

/// Get information about a Jira user
pub fn get_user(email: impl Display) -> Result<GetUserResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, get_user);
    }

    let request = email.to_string();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        jira_get_user(
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
    Ok(serde_json::from_str(&String::from_utf8(return_buffer).unwrap()).unwrap())
}

/// Post a comment to a Jira issue
pub fn post_comment(payload: PostCommentRequest) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(jira, post_comment);
    }

    let request = serde_json::to_string(&payload).unwrap();
    let res = unsafe { jira_post_comment(request.as_bytes().as_ptr(), request.as_bytes().len()) };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

/// Search for Jira issues
pub fn search_issues(
    payload: SearchIssueRequest,
) -> Result<SearchIssueResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, search_issues);
    }

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        jira_search_issues(
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
    Ok(serde_json::from_str(&String::from_utf8(return_buffer).unwrap()).unwrap())
}
