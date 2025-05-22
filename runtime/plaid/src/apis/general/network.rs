use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::loader::PlaidModule;
use reqwest::{header::HeaderMap, Certificate, Client};
use serde::{de, Deserialize, Serialize};

use crate::apis::ApiError;

use super::General;

#[derive(Deserialize)]
pub struct Config {
    pub web_requests: HashMap<String, Request>,
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
    /// Optional root TLS certificate to use for this request.  
    /// When set, the request will be sent via a special HTTP client configured with this certificate.
    #[serde(deserialize_with = "certificate_deserializer")]
    pub root_certificate: Option<Certificate>,
    /// Optional per‐request timeout.  
    /// When set, the request will be sent via a special HTTP client configured with this timeout;  
    /// if unset, the default timeout from the API config is used.
    #[serde(deserialize_with = "duration_deserializer")]
    pub timeout: Option<Duration>,
    /// Rules allowed to use this request
    allowed_rules: Vec<String>,
    /// Headers to include in the request
    headers: HashMap<String, String>,
    /// Whether the request is available to modules in test mode. Generally this should be set to false
    /// if the call has side effects. If it is not set, this will default to false.
    #[serde(default)]
    available_in_test_mode: bool,
}

/// Deserialize a non‐zero timeout (1–255 seconds) into a `Duration`, erroring on 0.
fn duration_deserializer<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let duration = u8::deserialize(deserializer)?;
    if duration == 0 {
        return Err(serde::de::Error::custom(format!(
            "Invalid timeout duration provided. Acceptable values are between 1 and 255 seconds"
        )));
    }

    Ok(Some(Duration::from_secs(duration as u64)))
}

/// Deserialize a PEM‐encoded string into a `Certificate`, erroring on parse failure.
fn certificate_deserializer<'de, D>(deserializer: D) -> Result<Option<Certificate>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let pem = String::deserialize(deserializer)?;
    let cert = Certificate::from_pem(pem.as_bytes()).map_err(|e| {
        serde::de::Error::custom(format!("Invalid certificate provided. Error: {e}"))
    })?;

    Ok(Some(cert))
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
            .clients
            .default
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

        let client = self.get_client(&request_name);
        let request_builder = match request_specification.verb.as_str() {
            "delete" => client.delete(&uri),
            "get" => client.get(&uri),
            "patch" => client.patch(&uri),
            "post" => client.post(&uri),
            "put" => client.put(&uri),
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

    fn get_client(&self, mnr: &str) -> &Client {
        if let Some(client) = self.clients.specialized.get(mnr) {
            client
        } else {
            &self.clients.default
        }
    }
}
