use std::fmt::Display;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use crate::apis::blockchain::evm::EvmCallError;

/// Possible JRPC methods we can call on nodes
pub enum RpcMethods {
    Call,
    SendRawTransaction,
    GasPrice,
    GetTransactionCount,
    GetTransactionReceipt,
    GetTransactionByHash,
    GetBalance,
    EstimateGas,
    GetLogs,
}

impl Display for RpcMethods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call => write!(f, "eth_call"),
            Self::SendRawTransaction => write!(f, "eth_sendRawTransaction"),
            Self::GasPrice => write!(f, "eth_gasPrice"),
            Self::GetTransactionCount => write!(f, "eth_getTransactionCount"),
            Self::GetTransactionReceipt => write!(f, "eth_getTransactionReceipt"),
            Self::GetTransactionByHash => write!(f, "eth_getTransactionByHash"),
            Self::GetBalance => write!(f, "eth_getBalance"),
            Self::EstimateGas => write!(f, "eth_estimateGas"),
            Self::GetLogs => write!(f, "eth_getLogs"),
        }
    }
}

impl Serialize for RpcMethods {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Serialize)]
pub struct JsonRpcRequest<'a> {
    pub jsonrpc: &'a str,
    pub method: RpcMethods,
    pub params: Value,
    pub id: u8,
}

impl JsonRpcRequest<'_> {
    pub fn new(method: RpcMethods, params: Option<Value>) -> Self {
        let params = params.map_or(Value::Array(vec![]), |param| param);

        Self {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        }
    }

    /// Make an arbitrary JRPC call
    pub async fn execute(&self, client: &Client, rpc: &str) -> Result<String, EvmCallError> {
        let body = serde_json::to_string(&self).map_err(EvmCallError::SerdeError)?;

        let response = client
            .post(rpc)
            .body(body)
            .send()
            .await
            .map_err(EvmCallError::NetworkError)?;

        if !response.status().is_success() {
            let status = response.status();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to get error message".to_string());
            return Err(EvmCallError::CallFailed { status, message });
        }

        response.text().await.map_err(EvmCallError::NetworkError)
    }
}
