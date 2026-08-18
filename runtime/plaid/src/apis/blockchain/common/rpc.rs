use reqwest::Client;
use serde::Serialize;

use crate::apis::blockchain::common::BlockchainError;

/// A JSON-RPC 2.0 request envelope, generic over the method type `M`.
///
/// Both EVM and Solana speak JSON-RPC 2.0 over HTTP; only the set of method
/// names differs. Each chain family supplies its own `method` enum (which must
/// be `Serialize` for the wire format and `Display` for logging).
#[derive(Serialize)]
pub struct JsonRpcRequest<'a, M: Serialize, P: Serialize> {
    pub jsonrpc: &'a str,
    pub method: M,
    pub params: Option<P>,
    pub id: u8,
}

impl<'a, M: Serialize, P: Serialize> JsonRpcRequest<'a, M, P> {
    pub fn new(method: M, params: Option<P>) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        }
    }

    /// Make an arbitrary JRPC call
    pub async fn execute(&self, client: &Client, rpc: &str) -> Result<String, BlockchainError> {
        let body = serde_json::to_string(&self).map_err(BlockchainError::SerdeError)?;

        let response = client
            .post(rpc)
            .body(body)
            .send()
            .await
            .map_err(BlockchainError::NetworkError)?;

        if !response.status().is_success() {
            let status = response.status();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to get error message".to_string());
            return Err(BlockchainError::CallFailed { status, message });
        }

        response.text().await.map_err(BlockchainError::NetworkError)
    }
}
