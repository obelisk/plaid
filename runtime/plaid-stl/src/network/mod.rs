use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::PlaidFunctionError;

#[derive(Serialize, Deserialize)]
pub struct MakeRequestRequest {
    /// Body of the request
    pub body: String,
    /// Name of the request - defined in the configuration
    pub request_name: String,
    /// Variables to include in the request. Variables take the place of an identifier in the request URI
    pub variables: HashMap<String, String>,
    /// Dynamic headers to include in the request. These are headers that cannot be statically
    /// defined in the request configuration. They cannot override a request's statically defined headers
    pub headers: Option<HashMap<String, String>>,
    /// Response encoding format
    pub response_encoding: MnrResponseEncoding,
}

#[derive(Deserialize, Serialize)]
pub enum MnrResponseEncoding {
    /// Response is UTF-8 encoded String
    Utf8,
    /// Response is unencoded binary data
    Binary,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct WebRequestResponse<T> {
    pub code: Option<u16>,
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert: Option<String>,
}

/// Request structure for retrieving a TLS certificate with SNI
#[derive(Serialize, Deserialize)]
pub struct TlsCertWithSniRequest {
    /// Domain of the TCP endpoint
    pub domain: String,
    /// SNI hostname
    pub sni: String,
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

pub fn make_named_request_with_buf_size<T: DeserializeOwned>(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
    headers: Option<HashMap<String, String>>,
    buffer_size: usize,
    response_encoding: MnrResponseEncoding,
) -> Result<WebRequestResponse<T>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(general, make_named_request);
    }

    let request = MakeRequestRequest {
        request_name: name.to_owned(),
        body: body.to_owned(),
        variables,
        headers,
        response_encoding,
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
) -> Result<WebRequestResponse<String>, PlaidFunctionError> {
    return make_named_request_with_buf_size(
        name,
        body,
        variables,
        None,
        RETURN_BUFFER_SIZE,
        MnrResponseEncoding::Utf8,
    );
}

/// Makes a named request and returns the response data in binary.
/// Use this function when expecting binary response data (images, files, etc.).
pub fn make_named_request_binary(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
) -> Result<WebRequestResponse<Vec<u8>>, PlaidFunctionError> {
    return make_named_request_with_buf_size(
        name,
        body,
        variables,
        None,
        RETURN_BUFFER_SIZE,
        MnrResponseEncoding::Binary,
    );
}

/// Enables calling of a named request with dynamic headers. This function should be used
/// when an API request requires header values that are created at runtime (example: HMAC authentication to another service).
/// Note: Headers included in this request can not override any statically defined headers in the request's config
pub fn make_named_request_with_headers(
    name: &str,
    body: &str,
    variables: HashMap<String, String>,
    headers: HashMap<String, String>,
) -> Result<WebRequestResponse<String>, PlaidFunctionError> {
    return make_named_request_with_buf_size(
        name,
        body,
        variables,
        Some(headers),
        RETURN_BUFFER_SIZE,
        MnrResponseEncoding::Utf8,
    );
}

/// Retrive a TLS certificate for a given domain, using a specified SNI (Server Name Indication).
pub fn retrieve_tls_certificate_with_sni(
    request: &TlsCertWithSniRequest,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(general, retrieve_tls_certificate_with_sni);
    }

    let params = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        general_retrieve_tls_certificate_with_sni(
            params.as_ptr(),
            params.len(),
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

    match String::from_utf8(return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
