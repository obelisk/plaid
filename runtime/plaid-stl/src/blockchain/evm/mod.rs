pub mod types;

use std::fmt::Display;

use crate::{
    blockchain::evm::types::{
        BasicRpcResponse, BlockTag, ChainId, DetailedRpcResponse, EstimateGasRequest,
        EthCallRequest, GetAddressMetadataRequest, GetGasPriceRequest, GetTransactionRequest,
    },
    PlaidFunctionError,
};

const DETAILED_RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
const BASIC_RETURN_BUFFER_SIZE: usize = 1024 * 10; // 10 KiB

/// Returns information about a transaction requested by transaction hash.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_gettransactionbyhash for more details.
pub fn get_transaction_by_hash(
    hash: impl Display,
    chain_id: impl Into<ChainId>,
) -> Result<DetailedRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, get_transaction_by_hash);
    }

    let request = GetTransactionRequest {
        chain_id: chain_id.into(),
        hash: hash.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; DETAILED_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_get_transaction_by_hash(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            DETAILED_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Returns the receipt of a transaction by transaction hash.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_gettransactionreceipt for more details.
pub fn get_transaction_receipt(
    hash: impl Display,
    chain_id: impl Into<ChainId>,
) -> Result<DetailedRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, get_transaction_receipt);
    }

    let request = GetTransactionRequest {
        chain_id: chain_id.into(),
        hash: hash.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; DETAILED_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_get_transaction_receipt(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            DETAILED_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Creates new message call transaction or a contract creation for signed transactions.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_sendrawtransaction for more details.
pub fn send_raw_transaction(
    signed_tx: impl Display,
    chain_id: impl Into<ChainId>,
) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, send_raw_transaction);
    }

    let request = types::SendRawTransactionRequest {
        chain_id: chain_id.into(),
        signed_tx: signed_tx.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_send_raw_transaction(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Returns the number of transactions sent from an address.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_gettransactioncount for more details.
pub fn get_transaction_count(
    address: impl Display,
    chain_id: impl Into<ChainId>,
    block_tag: BlockTag,
) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, get_transaction_count);
    }

    let request = GetAddressMetadataRequest {
        chain_id: chain_id.into(),
        address: address.to_string(),
        block_tag,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_get_transaction_count(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Returns the balance of an address.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_getbalance for more details.
pub fn get_balance(
    address: impl Display,
    chain_id: impl Into<ChainId>,
    block_tag: BlockTag,
) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, get_balance);
    }

    let request = GetAddressMetadataRequest {
        chain_id: chain_id.into(),
        address: address.to_string(),
        block_tag,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_get_balance(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Generates and returns an estimate of how much gas is necessary to allow the transaction to complete.
/// The transaction will not be added to the blockchain. Note that the estimate may be significantly more
/// than the amount of gas actually used by the transaction, for a variety of reasons including EVM mechanics and node performance.
///
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_estimategas for more details.
pub fn estimate_gas(request: EstimateGasRequest) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, estimate_gas);
    }

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_estimate_gas(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Executes a new message call immediately without creating a transaction on the blockchain.
/// Often used for executing read-only smart contract functions, for example the balanceOf for an ERC-20 contract.
///
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_call for more details.
pub fn eth_call(
    to: impl Display,
    data: impl Display,
    chain_id: impl Into<ChainId>,
    block_tag: BlockTag,
) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, eth_call);
    }

    let request = EthCallRequest {
        chain_id: chain_id.into(),
        block_tag,
        to: to.to_string(),
        data: data.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_eth_call(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Returns an estimate of the current price per gas in wei.
/// See https://ethereum.org/developers/docs/apis/json-rpc/#eth_gasprice for more details.
pub fn gas_price(chain_id: impl Into<ChainId>) -> Result<BasicRpcResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(blockchain_evm, gas_price);
    }

    let request = GetGasPriceRequest {
        chain_id: chain_id.into(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let mut return_buffer = vec![0; BASIC_RETURN_BUFFER_SIZE];

    let res = unsafe {
        blockchain_evm_gas_price(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            BASIC_RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match std::str::from_utf8(&return_buffer) {
        Ok(x) => Ok(serde_json::from_str(x).map_err(|_| PlaidFunctionError::InternalApiError)?),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
