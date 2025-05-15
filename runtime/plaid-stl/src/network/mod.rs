use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

#[derive(Deserialize)]
pub struct WebRequestResponse {
    pub code: Option<u16>,
    pub data: Option<String>,
}

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

pub fn simple_json_post_request(
    url: &str,
    body: &str,
    auth: Option<&str>,
) -> Result<u32, PlaidFunctionError> {
    extern "C" {
        new_host_function!(general, simple_json_post_request);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("url", url);
    params.insert("body", body);

    if let Some(auth) = auth {
        params.insert("auth", auth);
    }

    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { general_simple_json_post_request(params.as_ptr(), params.len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(res as u32)
}

pub fn make_named_request_with_buf_size(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
    headers: Option<HashMap<String, String>>,
    buffer_size: usize,
) -> Result<WebRequestResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(general, make_named_request);
    }

    #[derive(Serialize)]
    struct MakeRequestRequest {
        request_name: String,
        body: String,
        variables: HashMap<String, String>,
        headers: Option<HashMap<String, String>>,
    }

    let request = MakeRequestRequest {
        request_name: name.to_owned(),
        body: body.to_owned(),
        variables,
        headers,
    };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; buffer_size];

    let res = unsafe {
        general_make_named_request(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            buffer_size,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

pub fn make_named_request(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
) -> Result<WebRequestResponse, PlaidFunctionError> {
    return make_named_request_with_buf_size(name, body, variables, None, RETURN_BUFFER_SIZE);
}

/// Enables calling of a named request with dynamic headers. This function should be used
/// when an API request requires header values that are created at runtime (example: HMAC authentication to another service).
/// Note: Headers included in this request can not override any statically defined headers in the request's config
pub fn make_named_request_with_headers(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
    headers: HashMap<String, String>,
) -> Result<WebRequestResponse, PlaidFunctionError> {
    return make_named_request_with_buf_size(
        name,
        body,
        variables,
        Some(headers),
        RETURN_BUFFER_SIZE,
    );
}
