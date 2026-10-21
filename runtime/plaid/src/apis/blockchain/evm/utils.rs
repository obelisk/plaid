use std::fmt::Display;

use serde::Serialize;
use tiny_keccak::{Hasher, Keccak};

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
    GetBlock,
    GetFeeHistory,
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
            Self::GetBlock => write!(f, "eth_getBlockByNumber"),
            Self::GetFeeHistory => write!(f, "eth_feeHistory"),
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

/// Computes the Keccak-256 hash of a raw transaction and returns it as a `0x`-prefixed hex string.
pub fn compute_transaction_hash(raw_tx: &[u8]) -> String {
    let mut keccak = Keccak::v256();
    keccak.update(raw_tx);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    format!("0x{}", hex::encode(hash))
}
