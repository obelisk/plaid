//! # Error Handling Example
//!
//! Demonstrates production-quality error handling patterns for plaid rules,
//! matching the style used in real production rules like `bbqd`.
//!
//! ## Key concepts
//! - `thiserror` crate for ergonomic error enums with `#[from]` derives
//! - `plaid::set_error_context(msg)` — give the runtime more detail about
//!   what went wrong (appears in plaid's error logs)
//! - Mapping `PlaidFunctionError` to your own error type
//! - Separating error types into their own module (`error.rs`)
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."errors"]
//! log_type = "error_handling"
//! headers = ["Content-Type"]
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! # Valid request:
//! curl -s -X POST http://localhost:8080/webhook/errors \
//!   -H "Content-Type: application/json" \
//!   -d '{"value": 42}'
//!
//! # Trigger a parse error:
//! curl -s -X POST http://localhost:8080/webhook/errors \
//!   -d 'not json'
//!
//! # Trigger a validation error:
//! curl -s -X POST http://localhost:8080/webhook/errors \
//!   -H "Content-Type: application/json" \
//!   -d '{"value": -1}'
//!
//! # Trigger a storage error (value too large to display):
//! curl -s -X POST http://localhost:8080/webhook/errors \
//!   -H "Content-Type: application/json" \
//!   -d '{"value": 999}'
//! ```

mod error;

use error::Error;
use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

#[derive(Deserialize)]
struct Input {
    value: i64,
}

#[derive(Serialize)]
struct Output {
    result: String,
    stored: bool,
}

fn process(data: &str) -> Result<Output, Error> {
    // Step 1: Parse the input. serde_json::Error automatically converts
    // to our Error type via #[from].
    let input: Input = serde_json::from_str(data)?;

    // Step 2: Validate the input. This is a custom error variant.
    if input.value < 0 {
        return Err(Error::ValidationFailed(format!(
            "value must be non-negative, got {}",
            input.value
        )));
    }

    // Step 3: Interact with plaid APIs. PlaidFunctionError converts
    // to our Error type via the manual From impl in error.rs.
    let result = format!("processed_{}", input.value);

    // Step 4: Try to store the result. Demonstrate error context.
    let stored = match plaid::storage::insert("last_result", result.as_bytes()) {
        Ok(_) => true,
        Err(e) => {
            // set_error_context gives the runtime more detail about the error.
            // This appears in plaid's logs alongside the error code.
            plaid::set_error_context(&format!("storage insert failed: {e}"));
            false
        }
    };

    // Step 5: Simulate an error for large values (demonstrates error flow).
    if input.value > 100 {
        return Err(Error::ProcessingFailed(
            "value exceeds maximum threshold of 100".to_string(),
        ));
    }

    Ok(Output { result, stored })
}

fn main(data: String, _source: LogSource) -> Result<Option<String>, i32> {
    match process(&data) {
        Ok(output) => {
            let body = serde_json::to_string_pretty(&output).unwrap();
            Ok(Some(body))
        }
        Err(e) => {
            // Log the error for debugging. In production, this would go to
            // your monitoring system.
            plaid::print_debug_string(&format!("[error-handling] error: {e}"));
            plaid::set_error_context(&e.to_string());

            // Return an error response to the caller with details.
            let body = format!("{{\"error\": \"{e}\"}}");
            Ok(Some(body))
        }
    }
}
