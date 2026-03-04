//! # Echo JSON Example
//!
//! Demonstrates how to parse an incoming JSON webhook payload, extract fields,
//! and return a JSON response to the caller.
//!
//! ## Key concepts
//! - `entrypoint_with_source_and_response!()` — the entry point macro that lets
//!   your rule return a response string to the webhook caller
//! - Typed deserialization with `serde::Deserialize`
//! - Returning `Ok(Some(string))` to send a response body back
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."echo"]
//! log_type = "echo_json"
//! headers = ["Content-Type"]
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! curl -s -X POST http://localhost:8080/webhook/echo \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "alice", "age": 30}'
//! ```

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

/// The input we expect from the webhook caller.
#[derive(Deserialize)]
struct Input {
    name: String,
    #[serde(default)]
    age: Option<u32>,
}

/// The response we send back.
#[derive(Serialize)]
struct Output {
    greeting: String,
    received_at: u32,
    source: String,
}

fn main(data: String, source: LogSource) -> Result<Option<String>, i32> {
    plaid::print_debug_string(&format!("[echo-json] received {} bytes", data.len()));

    // Parse the incoming JSON. If it's not valid, return an error response.
    let input: Input = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            let err = format!("{{\"error\": \"invalid JSON: {e}\"}}");
            return Ok(Some(err));
        }
    };

    // Build a response with the parsed fields plus some metadata.
    let greeting = match input.age {
        Some(age) => format!("Hello, {}! You are {} years old.", input.name, age),
        None => format!("Hello, {}!", input.name),
    };

    let response = Output {
        greeting,
        received_at: plaid::get_time(),
        source: source.to_string(),
    };

    // Serialize and return. Returning Ok(Some(string)) sends the string
    // as the HTTP response body to the webhook caller.
    let body = serde_json::to_string_pretty(&response).unwrap();
    plaid::print_debug_string(&format!("[echo-json] responding with: {body}"));
    Ok(Some(body))
}
