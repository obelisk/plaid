//! # Request Logger Example
//!
//! Demonstrates how to access all the contextual data available to a rule:
//! HTTP headers, query parameters, accessory data from config, and secrets.
//!
//! ## Key concepts
//! - `plaid::get_headers(name)` — retrieve a forwarded HTTP header by name
//! - `plaid::get_query_params(name)` — retrieve a URL query parameter by name
//! - `plaid::get_accessory_data(name)` — retrieve static config data
//! - `plaid::get_secrets(name)` — retrieve secrets from secrets.toml
//! - Headers must be listed in the webhook config to be forwarded
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."inspect"]
//! log_type = "request_logger"
//! headers = ["Content-Type", "Authorization", "User-Agent", "X-Custom-Header"]
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! # POST with custom headers:
//! curl -s -X POST http://localhost:8080/webhook/inspect \
//!   -H "Content-Type: application/json" \
//!   -H "X-Custom-Header: my-value" \
//!   -H "Authorization: Bearer test-token" \
//!   -d '{"message": "hello"}'
//! ```

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::Serialize;

entrypoint_with_source_and_response!();

/// The headers we attempt to read. These must also be listed in the
/// webhook config's `headers` array to be forwarded by plaid.
const HEADER_NAMES: &[&str] = &[
    "Content-Type",
    "Authorization",
    "User-Agent",
    "X-Custom-Header",
];

#[derive(Serialize)]
struct InspectionResult {
    source: String,
    body_length: usize,
    headers: Vec<HeaderEntry>,
}

#[derive(Serialize)]
struct HeaderEntry {
    name: String,
    value: Option<String>,
}

fn main(data: String, source: LogSource) -> Result<Option<String>, i32> {
    plaid::print_debug_string(&format!("[request-logger] source={source}"));
    plaid::print_debug_string(&format!("[request-logger] body ({} bytes): {data}", data.len()));

    // Read each configured header. get_headers returns Err if the header
    // wasn't forwarded or wasn't present in the request.
    let mut headers = Vec::new();
    for &name in HEADER_NAMES {
        let value = plaid::get_headers(name).ok();
        plaid::print_debug_string(&format!("[request-logger] header {name}: {value:?}"));
        headers.push(HeaderEntry {
            name: name.to_string(),
            value,
        });
    }

    let result = InspectionResult {
        source: source.to_string(),
        body_length: data.len(),
        headers,
    };

    let body = serde_json::to_string_pretty(&result).unwrap();
    Ok(Some(body))
}
