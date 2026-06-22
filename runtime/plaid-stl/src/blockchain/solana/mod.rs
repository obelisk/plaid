pub mod types;
mod utils;

use std::borrow::Cow;

pub use utils::{parse_rpc_response, SolanaError};

use crate::blockchain::solana::types::{
    Cluster, ClusterRequest, GetBlockRequest, GetFeeForMessageRequest,
    GetMinimumBalanceForRentExemptionRequest, GetMultipleAccountsRequest,
    GetProgramAccountsRequest, GetRecentPrioritizationFeesRequest, GetSignatureStatusesRequest,
    GetSignaturesForAddressRequest, GetTokenAccountsByOwnerRequest, GetTransactionRequest,
    ProgramAccountsFilters, PubkeyRequest, SendTransactionRequest, SolanaRpcResponse,
    UnvalidatedPubkey, UnvalidatedSignature,
};
use crate::PlaidFunctionError;
use serde::Serialize;

/// Buffer size for Solana RPC responses.
///
/// Solana responses (accounts, blocks, program scans) can be large; `getProgramAccounts`
/// and `getBlock` against busy programs may exceed this. We ensure to error out in that case.
const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

/// Submits a fully-signed, base64-encoded transaction (`sendTransaction`).
pub fn send_signed_transaction(
    transaction: impl AsRef<str>,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, send_signed_transaction);
    }
    let request = serialize_request(&SendTransactionRequest {
        cluster,
        transaction: Cow::Borrowed(transaction.as_ref()),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_send_signed_transaction(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the lamport balance of an account (`getBalance`).
pub fn get_balance(
    pubkey: &UnvalidatedPubkey,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_balance);
    }
    let request = serialize_request(&PubkeyRequest {
        cluster,
        pubkey: Cow::Borrowed(pubkey),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_balance(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns all information associated with an account (`getAccountInfo`).
pub fn get_account_info(
    pubkey: &UnvalidatedPubkey,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_account_info);
    }
    let request = serialize_request(&PubkeyRequest {
        cluster,
        pubkey: Cow::Borrowed(pubkey),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_account_info(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the slot that has reached the default commitment level (`getSlot`).
pub fn get_slot(cluster: Cluster) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_slot);
    }
    let request = serialize_request(&ClusterRequest { cluster })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_slot(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the latest blockhash (`getLatestBlockhash`).
pub fn get_latest_blockhash(cluster: Cluster) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_latest_blockhash);
    }
    let request = serialize_request(&ClusterRequest { cluster })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_latest_blockhash(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the current cluster-wide transaction count (`getTransactionCount`).
pub fn get_transaction_count(cluster: Cluster) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_transaction_count);
    }
    let request = serialize_request(&ClusterRequest { cluster })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_transaction_count(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns details for a confirmed transaction by signature (`getTransaction`).
pub fn get_transaction(
    signature: &UnvalidatedSignature,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_transaction);
    }
    let request = serialize_request(&GetTransactionRequest {
        cluster,
        signature: Cow::Borrowed(signature),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_transaction(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the processing statuses of a batch of signatures (`getSignatureStatuses`).
///
/// Set `search_transaction_history` to `true` to scan the full ledger for signatures
/// that have left the recent status cache; this is expensive, so leave it
/// `false` for the common "is this recent signature confirmed?" check.
pub fn get_signature_statuses(
    signatures: &[UnvalidatedSignature],
    search_transaction_history: bool,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_signature_statuses);
    }
    let request = serialize_request(&GetSignatureStatusesRequest {
        cluster,
        signatures: Cow::Borrowed(signatures),
        search_transaction_history,
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_signature_statuses(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns identity and transaction information about a confirmed block (`getBlock`).
pub fn get_block(slot: u64, cluster: Cluster) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_block);
    }
    let request = serialize_request(&GetBlockRequest { cluster, slot })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_block(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Reads multiple accounts in one call (`getMultipleAccounts`).
pub fn get_multiple_accounts(
    pubkeys: &[UnvalidatedPubkey],
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_multiple_accounts);
    }
    let request = serialize_request(&GetMultipleAccountsRequest {
        cluster,
        pubkeys: Cow::Borrowed(pubkeys),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_multiple_accounts(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Enumerates the accounts owned by a program (`getProgramAccounts`).
///
/// Pass [`ProgramAccountsFilters`] to scope the scan by account size / `memcmp` and to
/// project a slice of each account's data. An empty (default) filter set performs an
/// unfiltered scan, which can return a large payload — see [`RETURN_BUFFER_SIZE`].
pub fn get_program_accounts(
    program_id: &UnvalidatedPubkey,
    filters: ProgramAccountsFilters,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_program_accounts);
    }
    let request = serialize_request(&GetProgramAccountsRequest {
        cluster,
        program_id: Cow::Borrowed(program_id),
        filters,
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_program_accounts(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Lists the SPL token accounts owned by a wallet (`getTokenAccountsByOwner`).
///
/// Provide `mint` to filter to a single mint, or `program_id` to scope to a token
/// program; if both are `None`, the host defaults to the SPL Token program.
pub fn get_token_accounts_by_owner(
    owner: &UnvalidatedPubkey,
    mint: Option<&UnvalidatedPubkey>,
    program_id: Option<&UnvalidatedPubkey>,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_token_accounts_by_owner);
    }
    let request = serialize_request(&GetTokenAccountsByOwnerRequest {
        cluster,
        owner: Cow::Borrowed(owner),
        mint: mint.map(Cow::Borrowed),
        program_id: program_id.map(Cow::Borrowed),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_token_accounts_by_owner(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the token balance of an SPL token account (`getTokenAccountBalance`).
pub fn get_token_account_balance(
    pubkey: &UnvalidatedPubkey,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_token_account_balance);
    }
    let request = serialize_request(&PubkeyRequest {
        cluster,
        pubkey: Cow::Borrowed(pubkey),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_token_account_balance(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the total supply of an SPL mint (`getTokenSupply`).
pub fn get_token_supply(
    mint: &UnvalidatedPubkey,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_token_supply);
    }
    let request = serialize_request(&PubkeyRequest {
        cluster,
        pubkey: Cow::Borrowed(mint),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_token_supply(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the lamports needed for an account of `data_length` bytes to be rent-exempt
/// (`getMinimumBalanceForRentExemption`).
pub fn get_minimum_balance_for_rent_exemption(
    data_length: u64,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(
            blockchain_solana,
            get_minimum_balance_for_rent_exemption
        );
    }
    let request = serialize_request(&GetMinimumBalanceForRentExemptionRequest {
        cluster,
        data_length,
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_minimum_balance_for_rent_exemption(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns the fee the cluster would charge for a serialized message (`getFeeForMessage`).
pub fn get_fee_for_message(
    message: impl AsRef<str>,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_fee_for_message);
    }
    let request = serialize_request(&GetFeeForMessageRequest {
        cluster,
        message: Cow::Borrowed(message.as_ref()),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_fee_for_message(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns recent prioritization fees, optionally scoped to `addresses`
/// (`getRecentPrioritizationFees`).
pub fn get_recent_prioritization_fees(
    addresses: &[UnvalidatedPubkey],
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_recent_prioritization_fees);
    }
    let request = serialize_request(&GetRecentPrioritizationFeesRequest {
        cluster,
        addresses: Cow::Borrowed(addresses),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_recent_prioritization_fees(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Simulates a signed transaction without submitting it (`simulateTransaction`).
pub fn simulate_transaction(
    transaction: impl AsRef<str>,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, simulate_transaction);
    }
    let request = serialize_request(&SendTransactionRequest {
        cluster,
        transaction: Cow::Borrowed(transaction.as_ref()),
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_simulate_transaction(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Returns signatures for transactions involving an address (`getSignaturesForAddress`).
pub fn get_signatures_for_address(
    address: &UnvalidatedPubkey,
    limit: Option<u64>,
    cluster: Cluster,
) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_solana, get_signatures_for_address);
    }
    let request = serialize_request(&GetSignaturesForAddressRequest {
        cluster,
        address: Cow::Borrowed(address),
        limit,
    })?;
    let mut buffer = vec![0; RETURN_BUFFER_SIZE];
    let res = unsafe {
        blockchain_solana_get_signatures_for_address(
            request.as_ptr(),
            request.len(),
            buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };
    finish(res, buffer)
}

/// Serialize a request type to the JSON the host expects.
fn serialize_request<R: Serialize>(request: &R) -> Result<String, PlaidFunctionError> {
    serde_json::to_string(request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)
}

/// Interpret the host return code + buffer as a parsed JSON-RPC envelope.
fn finish(res: i32, mut buffer: Vec<u8>) -> Result<SolanaRpcResponse, PlaidFunctionError> {
    if res < 0 {
        return Err(res.into());
    }
    buffer.truncate(res as usize);
    match std::str::from_utf8(&buffer) {
        Ok(x) => serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
