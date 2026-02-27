//! # Webhook Router Example
//!
//! Demonstrates how to chain rules together using the **logback** system.
//! This rule receives a JSON payload with a `target` field and forwards the
//! `body` to the appropriate downstream rule via `plaid::log_back()`.
//!
//! ## Key concepts
//! - `plaid::log_back(log_type, data, delay)` sends data to another rule
//! - The `log_type` must match a rule's configured log type
//! - `delay` is in seconds (0 = immediate)
//! - The webhook config must set `logbacks_allowed = "Unlimited"` (or a
//!   sufficient `Limited` value) for the rule to be allowed to logback
//! - Downstream rules see `LogSource::Logback(...)` as their source
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."router"]
//! log_type = "webhook_router"
//! headers = ["Content-Type"]
//! logbacks_allowed = "Unlimited"
//! ```
//!
//! ## Try it
//! ```sh
//! # Route to the hello-world rule:
//! curl -s -X POST http://localhost:8080/webhook/router \
//!   -H "Content-Type: application/json" \
//!   -d '{"target": "hello_world", "body": "routed from webhook-router!"}'
//!
//! # Route to the echo-json rule:
//! curl -s -X POST http://localhost:8080/webhook/router \
//!   -H "Content-Type: application/json" \
//!   -d '{"target": "echo_json", "body": "{\"name\": \"bob\"}"}'
//! ```

use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};
use serde::Deserialize;

entrypoint_with_source!();

#[derive(Deserialize)]
struct RouterRequest {
    /// The log_type of the downstream rule to forward to.
    target: String,
    /// The payload to send to the downstream rule.
    body: String,
    /// Optional delay in seconds before the downstream rule fires.
    #[serde(default)]
    delay: u32,
}

fn main(data: String, source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("[webhook-router] received from {source}"));

    let request: RouterRequest = serde_json::from_str(&data).map_err(|e| {
        plaid::print_debug_string(&format!("[webhook-router] invalid JSON: {e}"));
        1
    })?;

    plaid::print_debug_string(&format!(
        "[webhook-router] routing {} bytes to '{}' (delay={}s)",
        request.body.len(),
        request.target,
        request.delay,
    ));

    // log_back sends the body to whatever rule is listening on the given log_type.
    // The delay parameter specifies how many seconds to wait before delivering.
    plaid::log_back(&request.target, request.body.as_bytes(), request.delay)
        .map_err(|_| {
            plaid::print_debug_string("[webhook-router] logback failed — check logbacks_allowed config");
            1
        })?;

    plaid::print_debug_string("[webhook-router] logback sent successfully");
    Ok(())
}
