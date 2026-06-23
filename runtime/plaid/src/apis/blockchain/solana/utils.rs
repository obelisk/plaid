use std::fmt::Display;

use serde::Serialize;

/// JSON-RPC methods we can call on Solana nodes.
///
/// Mirrors `evm::utils::RpcMethods`. The generic `execute_rpc_call` requires the
/// method type to be `Serialize` (wire format) + `Display` (logging).
pub enum RpcMethods {
    SendTransaction,
    GetBalance,
    GetAccountInfo,
    GetSlot,
    GetLatestBlockhash,
    GetTransactionCount,
    GetTransaction,
    GetSignatureStatuses,
    GetBlock,
    GetMultipleAccounts,
    GetProgramAccounts,
    GetTokenAccountsByOwner,
    GetTokenAccountBalance,
    GetTokenSupply,
    GetMinimumBalanceForRentExemption,
    GetFeeForMessage,
    GetRecentPrioritizationFees,
    SimulateTransaction,
    GetSignaturesForAddress,
}

impl Display for RpcMethods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SendTransaction => write!(f, "sendTransaction"),
            Self::GetBalance => write!(f, "getBalance"),
            Self::GetAccountInfo => write!(f, "getAccountInfo"),
            Self::GetSlot => write!(f, "getSlot"),
            Self::GetLatestBlockhash => write!(f, "getLatestBlockhash"),
            Self::GetTransactionCount => write!(f, "getTransactionCount"),
            Self::GetTransaction => write!(f, "getTransaction"),
            Self::GetSignatureStatuses => write!(f, "getSignatureStatuses"),
            Self::GetBlock => write!(f, "getBlock"),
            Self::GetMultipleAccounts => write!(f, "getMultipleAccounts"),
            Self::GetProgramAccounts => write!(f, "getProgramAccounts"),
            Self::GetTokenAccountsByOwner => write!(f, "getTokenAccountsByOwner"),
            Self::GetTokenAccountBalance => write!(f, "getTokenAccountBalance"),
            Self::GetTokenSupply => write!(f, "getTokenSupply"),
            Self::GetMinimumBalanceForRentExemption => {
                write!(f, "getMinimumBalanceForRentExemption")
            }
            Self::GetFeeForMessage => write!(f, "getFeeForMessage"),
            Self::GetRecentPrioritizationFees => write!(f, "getRecentPrioritizationFees"),
            Self::SimulateTransaction => write!(f, "simulateTransaction"),
            Self::GetSignaturesForAddress => write!(f, "getSignaturesForAddress"),
        }
    }
}

// Could be derived, but this ensures it's in line with the Display impl.
impl Serialize for RpcMethods {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
