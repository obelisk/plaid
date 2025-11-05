mod utils;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

#[derive(Serialize, Deserialize)]
pub struct CreateIssueRequest {
    pub project_key: String,
    pub summary: String,
    pub description: String,
    // The caller might decide to use issuetype.id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuetype_name: Option<String>,
    // We use Value because sometimes Jira expects arrays or other objects
    #[serde(flatten)]
    pub other_fields: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateIssueResponse {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetIssueRequest {
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetIssueResponse {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_: String,
    pub fields: Value,
}

#[derive(Serialize, Deserialize)]
pub struct GetUserAccountIdRequest {
    pub email: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetUserAccountIdResponse {
    pub display_name: Option<String>,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct PostCommentRequest {
    pub issue_id: String,
    pub comment: String,
}

// ==============================================================================================================

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

pub fn get_issue(payload: GetIssueRequest) -> Result<GetIssueResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, get_issue);
    }

    let request = serde_json::to_string(&payload).unwrap();

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

pub fn get_user_id(
    payload: GetUserAccountIdRequest,
) -> Result<GetUserAccountIdResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(jira, get_user_id);
    }

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        jira_get_user_id(
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
