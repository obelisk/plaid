use serde::de::DeserializeOwned;

use crate::blockchain::evm::types::{BasicRpcResponse, DetailedRpcResponse};

#[derive(Debug)]
pub enum EvmError {
    FailedToDeserialize(serde_json::Error),
    RpcError {
        code: i32,
        message: String,
        data: Option<String>,
    },
    UnexpectedResponseFormat,
}

/// Parses a basic RPC response and returns the result as a string.
///
/// Returns an `EvmError` if the response contains an error or has an unexpected format.
pub fn parse_basic_rpc_response(response: BasicRpcResponse) -> Result<String, EvmError> {
    match (response.error, response.result) {
        (Some(error), _) => Err(EvmError::RpcError {
            code: error.code,
            message: error.message,
            data: error.data,
        }),
        (_, Some(data)) => Ok(data),
        _ => Err(EvmError::UnexpectedResponseFormat),
    }
}

/// Parses a detailed RPC response and deserializes the result into the given type.
///
/// Returns an `EvmError` if the response contains an error, cannot be deserialized,
/// or has an unexpected format.
pub fn parse_detailed_rpc_response<T: DeserializeOwned>(
    response: DetailedRpcResponse,
) -> Result<T, EvmError> {
    match (response.error, response.result) {
        (Some(error), _) => Err(EvmError::RpcError {
            code: error.code,
            message: error.message,
            data: error.data,
        }),
        (_, Some(data)) => serde_json::from_value(data).map_err(EvmError::FailedToDeserialize),
        _ => Err(EvmError::UnexpectedResponseFormat),
    }
}
