pub mod evm;
pub mod solana;

use serde::{Deserialize, Serialize};

/// Payload of the logback sent to the rule once a confirmed transaction
/// reaches a terminal state (or the confirmation attempt fails).
///
/// Chain-agnostic: shared by the EVM and Solana `confirm_transaction` APIs.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ConfirmTransactionResult {
    /// The outcome of the confirmation attempt.
    pub outcome: ConfirmOutcome,
    /// The data the rule passed to `confirm_transaction`, echoed back.
    pub additional_data: Option<serde_json::Value>,
    /// The transaction identifier this logback is about: the EVM tx hash or
    /// the Solana signature. Also returned by the `confirm_transaction` call
    /// itself, for correlation.
    pub tx_id: String,
}

/// The outcome of a transaction confirmation attempt. Mutually exclusive by
/// construction: a transaction either succeeded, failed on-chain, or the
/// confirmation attempt itself failed (e.g. timed out).
#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ConfirmOutcome {
    /// The transaction was included and executed successfully.
    Success,
    /// The transaction was included but reverted/failed on-chain.
    Failure,
    /// The confirmation attempt failed before a receipt was seen. The
    /// payload describes why (e.g. confirmation timed out after Ns).
    Error(String),
}
