use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::{apis::general::cert_sni::get_peer_certificate_with_sni, loader::PlaidModule};
use futures_util::stream::TryStreamExt;
use plaid_stl::network::{MakeRequestRequest, MnrResponseEncoding, TlsCertWithSniRequest};
use reqwest::{header::HeaderMap, Client};
use serde::{
    de::{self},
    Deserialize, Serialize,
};

use crate::apis::ApiError;

use super::General;

/// This enum is used to represent the data returned from a web request. It can be either text or binary data.
#[derive(Serialize)]
#[serde(untagged)]
enum ResponseData {
    Utf8(String),
    Binary(Vec<u8>),
}

/// This struct is used to represent the response from a web request. It contains the response code and the response data.
#[derive(Serialize)]
struct DynamicWebRequestResponse {
    /// Response code (e.g., 200, 404, etc.)
    code: Option<u16>,
    /// Response data, which can be either text or binary
    data: Option<ResponseData>,
    /// Peer certificate from the server
    #[serde(skip_serializing_if = "Option::is_none")]
    cert: Option<String>,
}

#[derive(Deserialize)]
pub struct Config {
    pub web_requests: HashMap<String, Request>,
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
    /// Flag to return the peer certificate from the server
    #[serde(default)] // default to false
    pub return_cert: bool,
    /// Optional root TLS certificate to use for this request.  
    /// When set, the request will be sent via a special HTTP client configured with this certificate.
    #[serde(default, deserialize_with = "certificate_deserializer")]
    pub root_certificate: Option<reqwest::Certificate>,
    /// Optional per‐request timeout.  
    /// When set, the request will be sent via a special HTTP client configured with this timeout;  
    /// if unset, the default timeout from the API config is used.
    #[serde(default, deserialize_with = "duration_deserializer")]
    pub timeout: Option<Duration>,
    /// Rules allowed to use this request
    allowed_rules: Vec<String>,
    /// Headers to include in the request
    headers: HashMap<String, String>,
    /// Whether the request is available to modules in test mode. Generally this should be set to false
    /// if the call has side effects. If it is not set, this will default to false.
    #[serde(default)]
    available_in_test_mode: bool,
    /// Whether to follow redirects
    #[serde(default)] // default to false
    pub enable_redirects: bool,
    /// The max size for the response body. If none is provided, default to no limit.
    pub max_response_size: Option<usize>,
}

/// Deserialize a non‐zero timeout (1–255 seconds) into a `Duration`, erroring on 0.
fn duration_deserializer<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let duration = Option::<u8>::deserialize(deserializer)?;
    match duration {
        None => Ok(None),
        Some(0) => Err(de::Error::custom(
            "Invalid timeout duration provided. Acceptable values are between 1 and 255 seconds",
        )),
        Some(secs) => Ok(Some(Duration::from_secs(secs as u64))),
    }
}

/// Deserialize a PEM‐encoded string into a `Certificate`, erroring on parse failure.
fn certificate_deserializer<'de, D>(
    deserializer: D,
) -> Result<Option<reqwest::Certificate>, D::Error>
where
    D: de::Deserializer<'de>,
{
    Option::<&[u8]>::deserialize(deserializer)?
        .map(reqwest::Certificate::from_pem)
        .transpose()
        .map_err(|e| serde::de::Error::custom(format!("Invalid certificate provided. Error: {e}")))
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

    /// This function checks if an MNR is allowed to be executed by the calling module.
    /// It returns Ok(()) if it can, otherwise it will return a sensible ApiError.
    fn make_named_request_check_permissions(
        &self,
        request_name: &str,
        module: Arc<PlaidModule>,
    ) -> Result<&Request, ApiError> {
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
            error!(
            "{module} tried to use web-request which is not available in test mode: {request_name}"
        );
            return Err(ApiError::TestMode);
        }

        Ok(request_specification)
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

        // Check that this module is allowed to use this MNR
        let request_specification =
            self.make_named_request_check_permissions(request_name, module)?;

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
            "head" => client.head(&request_specification.uri),
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
                let mut ret = DynamicWebRequestResponse {
                    code: None,
                    data: None,
                    cert: None,
                };

                if request_specification.return_code {
                    ret.code = Some(r.status().as_u16());
                }

                if request_specification.return_cert {
                    if let Some(certs) = r.extensions().get::<reqwest::tls::TlsInfo>() {
                        ret.cert = certs.peer_certificate().map(base64::encode);
                    }
                }

                if request_specification.return_body {
                    // Read the response body as a stream of bytes, checking we do not go beyond
                    // the max response size (if one is configured).
                    let mut body_stream = r.bytes_stream();
                    let mut body = Vec::new();
                    let mut total = 0;
                    while let Some(chunk) = body_stream
                        .try_next()
                        .await
                        .map_err(|e| ApiError::NetworkError(e))?
                    {
                        total += chunk.len();
                        if let Some(max_body_size) = request_specification.max_response_size {
                            if total > max_body_size {
                                return Err(ApiError::NetworkResponseTooLarge);
                            }
                        }
                        body.extend_from_slice(&chunk);
                    }

                    match request.response_encoding {
                        MnrResponseEncoding::Utf8 => {
                            let data = String::from_utf8(body).unwrap_or_default();
                            ret.data = Some(ResponseData::Utf8(data));
                        }
                        MnrResponseEncoding::Binary => {
                            ret.data = Some(ResponseData::Binary(body));
                        }
                    };
                }

                // TODO: This is not really a BadRequest.
                serde_json::to_string(&ret).map_err(|_| ApiError::BadRequest)
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    /// Retrieve a TLS certificate for a given domain, using a specified SNI (Server Name Indication).
    pub async fn retrieve_tls_certificate_with_sni(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: TlsCertWithSniRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Validating the request parameters here is not trivial and also does not seem strictly necessary,
        // since Plaid will only make a TLS connection to the given domain.

        info!(
            "Retrieving TLS certificate for domain [{}] using SNI [{}] on behalf of module [{}]",
            request.domain, request.sni, module
        );

        let cert = get_peer_certificate_with_sni(&request.domain, &request.sni)
            .await
            .map_err(|e| {
                error!("Error while retrieving TLS certificate: {e}");
                ApiError::TlsError(e)
            })?;

        Ok(cert)
    }

    fn get_client(&self, mnr: &str) -> &Client {
        if let Some(client) = self.clients.specialized.get(mnr) {
            client
        } else {
            &self.clients.default
        }
    }
}
