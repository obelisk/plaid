use serde::de::DeserializeOwned;

use crate::blockchain::solana::types::SolanaRpcResponse;

/// Errors surfaced when interpreting a Solana JSON-RPC response on the guest side.
#[derive(Debug)]
pub enum SolanaError {
    /// The `result` could not be deserialized into the requested type.
    FailedToDeserialize(serde_json::Error),
    /// The node returned a JSON-RPC error.
    RpcError {
        code: i32,
        message: String,
        data: Option<serde_json::Value>,
    },
    /// The response had neither a `result` nor an `error`.
    UnexpectedResponseFormat,
}

/// Extracts and deserializes the `result` of a Solana JSON-RPC response into `T`.
pub fn parse_rpc_response<T: DeserializeOwned>(
    response: SolanaRpcResponse,
) -> Result<T, SolanaError> {
    match (response.error, response.result) {
        (Some(error), _) => Err(SolanaError::RpcError {
            code: error.code,
            message: error.message,
            data: error.data,
        }),
        (_, Some(result)) => {
            serde_json::from_value(result).map_err(SolanaError::FailedToDeserialize)
        }
        _ => Err(SolanaError::UnexpectedResponseFormat),
    }
}
