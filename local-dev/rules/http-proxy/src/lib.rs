//! # HTTP Proxy Example
//!
//! Demonstrates how to make **outbound HTTP requests** from a plaid rule using
//! the named request system. Named requests are pre-configured in `apis.toml`
//! with a fixed URL, verb, and headers. The rule references them by name.
//!
//! This example uses [httpbin.org](https://httpbin.org) as a safe, public
//! endpoint that echoes back request details.
//!
//! ## Key concepts
//! - Named requests are defined in `apis.toml` under `[apis."general".network.web_requests]`
//! - Each named request specifies: verb, uri, allowed_rules, return_body/code
//! - `network::make_named_request(name, body, variables)` executes the request
//! - The response includes an optional HTTP status code and response body
//! - `variables` are substituted into the URI template (e.g., `{id}` in the URL)
//!
//! ## Config required
//! ```toml
//! # apis.toml
//! [apis."general".network.web_requests."httpbin_get"]
//! verb = "get"
//! uri = "https://httpbin.org/get"
//! return_body = true
//! return_code = true
//! return_cert = false
//! allowed_rules = ["http_proxy.wasm"]
//! [apis."general".network.web_requests."httpbin_get".headers]
//!
//! [apis."general".network.web_requests."httpbin_post"]
//! verb = "post"
//! uri = "https://httpbin.org/post"
//! return_body = true
//! return_code = true
//! return_cert = false
//! allowed_rules = ["http_proxy.wasm"]
//! [apis."general".network.web_requests."httpbin_post".headers]
//! Content-Type = "application/json"
//! ```
//!
//! ## Try it
//! ```sh
//! # GET request:
//! curl -s -X POST http://localhost:8080/webhook/proxy \
//!   -H "Content-Type: application/json" \
//!   -d '{"method": "get"}'
//!
//! # POST request with body:
//! curl -s -X POST http://localhost:8080/webhook/proxy \
//!   -H "Content-Type: application/json" \
//!   -d '{"method": "post", "body": "{\"hello\": \"world\"}"}'
//! ```

use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, network, plaid};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

#[derive(Deserialize)]
struct ProxyRequest {
    method: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Serialize)]
struct ProxyResponse {
    status_code: Option<u16>,
    body: Option<String>,
}

fn main(data: String, _source: LogSource) -> Result<Option<String>, i32> {
    let request: ProxyRequest = match serde_json::from_str(&data) {
        Ok(r) => r,
        Err(e) => return Ok(Some(format!("{{\"error\": \"invalid JSON: {e}\"}}"))),
    };

    // Choose the named request based on the method field.
    let request_name = match request.method.as_str() {
        "get" => "httpbin_get",
        "post" => "httpbin_post",
        other => {
            return Ok(Some(format!("{{\"error\": \"unsupported method: {other}\"}}")));
        }
    };

    let body = request.body.unwrap_or_default();
    let variables: HashMap<String, String> = HashMap::new();

    plaid::print_debug_string(&format!(
        "[http-proxy] making named request '{request_name}' with {} byte body",
        body.len()
    ));

    // make_named_request returns a WebRequestResponse with optional code and data.
    let response = network::make_named_request(request_name, &body, variables).map_err(|e| {
        plaid::print_debug_string(&format!("[http-proxy] request failed: {e}"));
        1
    })?;

    plaid::print_debug_string(&format!(
        "[http-proxy] response: code={:?} body_len={:?}",
        response.code,
        response.data.as_ref().map(|d| d.len())
    ));

    let proxy_response = ProxyResponse {
        status_code: response.code,
        body: response.data,
    };

    let result = serde_json::to_string_pretty(&proxy_response).unwrap();
    Ok(Some(result))
}
