use std::{collections::HashMap, sync::Arc};

use crate::loader::PlaidModule;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::apis::ApiError;

use super::General;

#[derive(Deserialize)]
pub struct Config {
    web_requests: HashMap<String, Request>,
}

/// Request to make a web request
#[derive(Deserialize)]
struct MakeRequestRequest {
    /// Body of the request
    body: String,
    /// Name of the request - defined in the configuration
    request_name: String,
    /// Variables to include in the request. Variables take the place of an idenfitifer in the request URI
    variables: HashMap<String, String>,
    /// Dynamic headers to include in the request. These are headers that cannot be statically
    /// defined in the request configuration. They cannot override a request's statically defined headers
    headers: Option<HashMap<String, String>>,
}

/// This struct represents a web request and contains information about what the request is about (e.g., verb and URI),
/// how it should be processed, and which modules are allowed to make it.
#[derive(Deserialize)]
pub struct Request {
    /// HTTP verb
    verb: String,
    /// Location to send the request
    uri: String,
    /// Body to include in the request
    body: Option<String>,
    /// Flag to return the body from the request
    return_body: bool,
    /// Flag to return the code from the request
    return_code: bool,
    /// Rules allowed to use this request
    allowed_rules: Vec<String>,
    /// Headers to include in the request
    headers: HashMap<String, String>,
    /// Whether the request is available to modules in test mode. Generally this should be set to false
    /// if the call has side effects. If it is not set, this will default to false.
    #[serde(default)]
    available_in_test_mode: bool,
}

/// Data returned by a request.
#[derive(Serialize)]
struct ReturnData {
    code: Option<u16>,
    data: Option<String>,
}

impl General {
    /// Make a post to a given address but don't provide any data returned. Only
    /// if the call returned 200
    ///
    /// This function should be considered an unsafe function because it allows arbitrary calls
    /// outside of normal sandboxing operations. This should only be used if `make_named_request`
    /// cannot be used.
    pub async fn simple_json_post_request(
        &self,
        params: &str,
        _: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let url = request
            .get("url")
            .ok_or(ApiError::MissingParameter("url".to_string()))?;
        let body = request
            .get("body")
            .ok_or(ApiError::MissingParameter("body".to_string()))?;
        let auth = request.get("auth");

        let request_builder = self
            .client
            .post(url)
            .header("Content-Type", "application/json; charset=utf-8");

        let request_builder = if let Some(auth) = auth {
            request_builder.header("Authorization", auth)
        } else {
            request_builder
        };

        match request_builder.body(body.clone()).send().await {
            Ok(r) => Ok(r.status().as_u16() as u32),
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    /// Make a named web request on behalf of a given `module`. The request's details are encoded in `params`.
    pub async fn make_named_request(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request: MakeRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let request_name = &request.request_name;

        // TODO: Log these failures better in the runtime
        let request_specification = match self.config.network.web_requests.get(request_name) {
            Some(x) => x,
            None => {
                error!("{module} tried to use web-request which doesn't exist: {request_name}");
                return Err(ApiError::BadRequest);
            }
        };

        // If this request is not allowed to be executed by the given rule, bail
        if !request_specification
            .allowed_rules
            .contains(&module.to_string())
        {
            error!("{module} tried to use web-request which it's not allowed to: {request_name}");
            return Err(ApiError::BadRequest);
        }

        // If the call is coming from a module in test mode, and the request is not allowed to be
        // called in test mode, return TestMode error
        if module.test_mode && !request_specification.available_in_test_mode {
            error!("{module} tried to use web-request which is not available in test mode: {request_name}");
            return Err(ApiError::TestMode);
        }

        let headers_to_include_in_request = match request.headers {
            Some(mut dynamic_headers) => {
                let static_headers = request_specification.headers.clone();
                dynamic_headers.extend(static_headers);
                dynamic_headers
            }
            None => request_specification.headers.clone(),
        };

        let headers: HeaderMap = match (&headers_to_include_in_request).try_into() {
            Ok(x) => x,
            Err(e) => {
                return Err(ApiError::ConfigurationError(format!(
                    "{request_name} has the headers misconfigured: {e}"
                )))
            }
        };

        let mut uri = request_specification.uri.clone();

        for replacement in request.variables.iter() {
            uri = uri.replace(format!("{{{}}}", replacement.0).as_str(), replacement.1);
        }

        let request_builder = match request_specification.verb.as_str() {
            "delete" => self.client.delete(&uri),
            "get" => self.client.get(&uri),
            "patch" => self.client.patch(&uri),
            "post" => self.client.post(&uri),
            "put" => self.client.put(&uri),
            // Not sure we want to support head
            //"head" => self.client.head(&request_specification.uri),
            _ => return Err(ApiError::BadRequest),
        }
        .headers(headers);

        // A body (even an empty one) must be provided. It can be overriden by
        // the configuration or provided by the rule. But if no body is defined
        // in both, this API will error.
        let body = match &request_specification.body {
            Some(x) => x.to_owned(),
            None => request.body,
        };

        match request_builder.body(body).send().await {
            Ok(r) => {
                let mut ret = ReturnData {
                    code: None,
                    data: None,
                };

                if request_specification.return_code {
                    ret.code = Some(r.status().as_u16());
                }

                if request_specification.return_body {
                    ret.data = Some(r.text().await.unwrap_or_default());
                }

                if let Ok(r) = serde_json::to_string(&ret) {
                    Ok(r)
                } else {
                    // TODO: This is not really a BadRequest.
                    Err(ApiError::BadRequest)
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}
