use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 4 * 1024; // 4 KiB

#[derive(Serialize, Deserialize)]
pub struct JwtParams {
    /// The ID of the key used to sign the JWT
    pub kid: String,
    /// Subject
    pub sub: String,
    /// Issued at
    pub iat: Option<u64>,
    /// Expiration
    pub exp: Option<u64>,
    /// Audience
    pub aud: Option<String>,
    /// Additional headers that will be validated before being included in the final JWT
    pub extra_headers: Option<HashMap<String, Value>>,
    /// Additional fields that will be validated before being included in the final JWT
    pub extra_fields: Option<HashMap<String, Value>>,
}

pub fn issue_jwt(request_params: &JwtParams) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(web, issue_jwt);
    }

    let params = serde_json::to_string(request_params).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        web_issue_jwt(
            params.as_ptr(),
            params.len(),
            return_buffer.as_mut_ptr(),
            return_buffer.len(),
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
