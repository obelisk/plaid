//! # Rate Limiter Example
//!
//! Demonstrates a practical pattern: using the in-memory **cache** to implement
//! per-key rate limiting. Each key gets a fixed number of requests; once
//! exceeded, the rule returns an error response.
//!
//! ## Key concepts
//! - Cache as a lightweight counter (no TTL in plaid cache, but values reset
//!   on restart — for true TTL, use storage with timestamps)
//! - Returning error responses to webhook callers via the response entrypoint
//! - Pattern: read-increment-write with cache
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."rate-limit"]
//! log_type = "rate_limiter"
//! headers = ["Content-Type"]
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! # Call 6 times — first 5 succeed, 6th is rate limited:
//! for i in $(seq 1 6); do
//!   echo "--- Request $i ---"
//!   curl -s -X POST http://localhost:8080/webhook/rate-limit \
//!     -H "Content-Type: application/json" \
//!     -d '{"key": "user-alice"}'
//!   echo
//! done
//! ```

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

/// Maximum requests allowed per key before rate limiting kicks in.
const MAX_REQUESTS: u64 = 5;

#[derive(Deserialize)]
struct Request {
    key: String,
}

#[derive(Serialize)]
struct RateLimitResponse {
    key: String,
    request_count: u64,
    limit: u64,
    allowed: bool,
}

fn main(data: String, _source: LogSource) -> Result<Option<String>, i32> {
    let request: Request = match serde_json::from_str(&data) {
        Ok(r) => r,
        Err(e) => {
            let err = format!("{{\"error\": \"invalid JSON: {e}\"}}");
            return Ok(Some(err));
        }
    };

    // Read the current count for this key from cache.
    let count = match plaid::cache::get(&request.key) {
        Ok(s) => s.parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    };

    let new_count = count + 1;
    let allowed = new_count <= MAX_REQUESTS;

    // Always update the counter, even if rate limited.
    let _ = plaid::cache::insert(&request.key, &new_count.to_string());

    let response = RateLimitResponse {
        key: request.key,
        request_count: new_count,
        limit: MAX_REQUESTS,
        allowed,
    };

    if !allowed {
        plaid::print_debug_string(&format!(
            "[rate-limiter] DENIED: {} has {} requests (limit {})",
            response.key, new_count, MAX_REQUESTS
        ));
    }

    let body = serde_json::to_string_pretty(&response).unwrap();
    Ok(Some(body))
}
