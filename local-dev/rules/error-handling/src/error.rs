//! Error types for the error-handling example.
//!
//! This pattern is used in production plaid rules (e.g., `bbqd`):
//! - Define an error enum with `thiserror::Error`
//! - Use `#[from]` for automatic conversion from library errors
//! - Implement `From<PlaidFunctionError>` manually since it comes from the STL

use plaid_stl::PlaidFunctionError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// JSON parsing failed. The `#[from]` attribute means `serde_json::Error`
    /// is automatically converted to this variant via the `?` operator.
    #[error("failed to parse JSON: {0}")]
    ParseError(#[from] serde_json::Error),

    /// Custom validation error.
    #[error("validation failed: {0}")]
    ValidationFailed(String),

    /// A plaid runtime API call failed.
    #[error("plaid API error: {0}")]
    PlaidError(String),

    /// Application-level processing error.
    #[error("processing failed: {0}")]
    ProcessingFailed(String),
}

/// Manual conversion from PlaidFunctionError since it doesn't implement
/// std::error::Error (it's defined in the WASM-targeted STL).
impl From<PlaidFunctionError> for Error {
    fn from(e: PlaidFunctionError) -> Self {
        Error::PlaidError(e.to_string())
    }
}
