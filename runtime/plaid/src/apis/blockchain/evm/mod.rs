mod utils;

use crate::{
    apis::{
        blockchain::{
            common::{
                rpc::JsonRpcRequest, BlockchainClient, BlockchainError, ChainFamily, NoOptions,
            },
            evm::utils::{compute_transaction_hash, RpcMethods},
        },
        ApiError,
    },
    loader::PlaidModule,
};
use log::{debug, warn};
use plaid_stl::blockchain::evm::types::{
    BasicRpcResponse, ChainId, ConfirmTransactionOptions, ConfirmTransactionRequest,
    DetailedRpcResponse, EstimateGasRequest, EthCallRequest, GetAddressMetadataRequest,
    GetBlockRequest, GetFeeHistoryRequest, GetGasPriceRequest, GetLogsRequest,
    GetTransactionRequest, JsonRpcError, SendRawTransactionRequest, TransactionReceipt,
};
use serde_json::{json, Number, Value};
use std::sync::Arc;
use std::time::Duration;

pub struct Evm;

impl ChainFamily for Evm {
    type Identifier = ChainId;
    type Options = NoOptions;
}

/// Minimum interval between receipt polls, so a misconfigured module cannot
/// hammer the node pool in a tight loop.
const MIN_POLL_INTERVAL_MS: u16 = 100;

/// Upper bound on the total time a `confirm_transaction` call may spend sleeping
/// (`max_poll_attempts * poll_interval_ms`). This host call blocks a runtime
/// thread while the module waits, so the window must stay bounded.
const MAX_CONFIRMATION_WINDOW_MS: u64 = 60_000;

/// EVM-specific error conditions, carried by [`BlockchainError::Evm`].
#[derive(Debug)]
pub enum EvmError {
    /// The node rejected the broadcast transaction with an RPC-level error.
    BroadcastRejected(JsonRpcError),
    /// The transaction was broadcast, but no receipt appeared within the poll window.
    ConfirmationTimeout {
        transaction_hash: String,
        poll_attempts: u8,
    },
    /// The node returned an RPC-level error while polling for the transaction receipt.
    ReceiptQueryFailed(JsonRpcError),
}

impl BlockchainClient<Evm> {
    /// Returns the information about a transaction requested by transaction hash.
    pub async fn get_transaction_by_hash(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionByHash, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns the receipt of a transaction by transaction hash.
    ///
    /// Note That the receipt is not available for pending transactions.
    pub async fn get_transaction_receipt(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionReceipt, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Creates new message call transaction or a contract creation for signed transactions.
    pub async fn send_raw_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<SendRawTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = Value::Array(vec![Value::String(request.signed_tx)]);
        let request = JsonRpcRequest::new(RpcMethods::SendRawTransaction, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns the number of transactions sent from an address.
    pub async fn get_transaction_count(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetAddressMetadataRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionCount, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns the balance of the account at a given address.
    pub async fn get_balance(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetAddressMetadataRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetBalance, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Generates and returns an estimate of how much gas is necessary to allow the transaction to complete.
    pub async fn estimate_gas(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<EstimateGasRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let mut object = serde_json::Map::new();
        if let Some(from) = request.from {
            object.insert("from".to_string(), Value::String(from.to_string()));
        }

        if let Some(to) = request.to {
            object.insert("to".to_string(), Value::String(to.to_string()));
        }
        if let Some(value) = request.value {
            object.insert("value".to_string(), Value::String(value.to_string()));
        }
        if let Some(data) = request.data {
            object.insert("data".to_string(), Value::String(data.to_string()));
        }
        let params = Value::Array(vec![
            Value::Object(object),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::EstimateGas, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Executes a new message call immediately without creating a transaction on the blockchain.
    /// Often used for executing read-only smart contract functions, for example the balanceOf for an ERC-20 contract.
    pub async fn eth_call(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<EthCallRequest>(params).map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let object = json!({ "to": request.to, "data": request.data });
        let params = Value::Array(vec![object, Value::String(request.block_tag.to_string())]);
        let request = JsonRpcRequest::new(RpcMethods::Call, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns an estimate of the current price per gas in wei. For example, the Besu client examines the last 100 blocks and returns the median gas unit price by default.
    pub async fn gas_price(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetGasPriceRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let request = JsonRpcRequest::<_, ()>::new(RpcMethods::GasPrice, None);

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns an array of all logs matching a given filter object.
    pub async fn get_logs(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetLogsRequest>(params).map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let mut object = serde_json::Map::new();
        object.insert(
            "fromBlock".to_string(),
            Value::String(request.from_block.to_string()),
        );
        object.insert(
            "toBlock".to_string(),
            Value::String(request.to_block.to_string()),
        );

        if let Some(addresses) = request.address {
            let val = if addresses.len() == 1 {
                Value::String(addresses[0].to_string())
            } else {
                Value::Array(
                    addresses
                        .iter()
                        .map(|a| Value::String(a.to_string()))
                        .collect(),
                )
            };
            object.insert("address".to_string(), val);
        }

        if let Some(topics) = request.topics {
            let topics_value = Value::Array(
                topics
                    .iter()
                    .map(|t| Value::String(t.to_string()))
                    .collect(),
            );
            object.insert("topics".to_string(), topics_value);
        };
        let params = serde_json::Value::Array(vec![Value::Object(object)]);

        let request = JsonRpcRequest::new(RpcMethods::GetLogs, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns information about a block by block number or tag.
    pub async fn get_block(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetBlockRequest>(params).map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let params = serde_json::Value::Array(vec![
            Value::String(request.block_tag.to_string()),
            Value::Bool(request.hydrated_transactions),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetBlock, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Returns transaction base fee per gas and effective priority fee per gas for the requested block range.
    pub async fn get_fee_history(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetFeeHistoryRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        let percentiles = request
            .reward_percentiles
            .unwrap_or_default()
            .into_iter()
            .map(|p| Value::Number(Number::from(p as u16)))
            .collect::<Vec<_>>();

        let params = vec![
            Value::String(format!("0x{:x}", request.block_count)),
            Value::String(request.block_tag.to_string()),
            Value::Array(percentiles),
        ];

        let params = Value::Array(params);
        let request = JsonRpcRequest::new(RpcMethods::GetFeeHistory, Some(params));

        self.execute_rpc_call(chain_id, request, module).await
    }

    /// Broadcasts a signed transaction and polls for its receipt until it is
    /// mined or the poll budget is exhausted.
    pub async fn confirm_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let ConfirmTransactionRequest {
            chain_id,
            signed_transaction,
            options,
        } = serde_json::from_str::<ConfirmTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;

        validate_poll_options(&module, &options)?;

        let tx_decoded = hex::decode(signed_transaction.trim_start_matches("0x"))
            .map_err(|_| ApiError::BadRequest)?;

        let expected_hash = compute_transaction_hash(&tx_decoded);

        // Broadcast the transaction. An RPC-level error from the node is a
        // definitive rejection; transport-level failures are ambiguous (the node
        // may have received and gossiped the transaction before the response was
        // lost) and fall through to polling.
        let broadcast_request = serde_json::to_string(&SendRawTransactionRequest {
            chain_id,
            signed_tx: signed_transaction,
        })
        .map_err(BlockchainError::SerdeError)?;

        match self
            .send_raw_transaction(&broadcast_request, module.clone())
            .await
        {
            Ok(response) => {
                let response = serde_json::from_str::<BasicRpcResponse>(&response)
                    .map_err(BlockchainError::SerdeError)?;

                if let Some(error) = response.error {
                    return Err(BlockchainError::Evm(EvmError::BroadcastRejected(error)).into());
                }
            }
            // Transport-level failures are ambiguous: the node may have received
            // and gossiped the transaction before the response was lost, so fall
            // through to polling rather than failing the request.
            Err(ApiError::BlockchainError(
                e @ (BlockchainError::NetworkError(_) | BlockchainError::AllNodesFailed),
            )) => {
                warn!(
                    "Module [{module}] broadcast transaction [{expected_hash}] on [{chain_id}] but the response was lost ({e:?}); polling for its receipt anyway"
                );
            }
            Err(e) => return Err(e),
        }

        // Poll for transaction status. If we haven't gotten anything back from
        // getTransactionReceipt, it stands to reason that it hasn't been
        // processed. Keep attempting until we exhaust our retries.
        let receipt_request = serde_json::to_string(&GetTransactionRequest {
            chain_id,
            hash: expected_hash.clone(),
        })
        .map_err(BlockchainError::SerdeError)?;

        let sleep_duration = Duration::from_millis(u64::from(options.poll_interval_ms));
        for attempt in 1..=options.max_poll_attempts {
            tokio::time::sleep(sleep_duration).await;

            match self
                .get_transaction_receipt(&receipt_request, module.clone())
                .await
            {
                Ok(response) => {
                    let response = serde_json::from_str::<DetailedRpcResponse>(&response)
                        .map_err(BlockchainError::SerdeError)?;

                    match (response.error, response.result) {
                        (Some(error), _) => {
                            return Err(
                                BlockchainError::Evm(EvmError::ReceiptQueryFailed(error)).into()
                            );
                        }
                        // The transaction has not been mined yet (`result: null`).
                        (None, None) => {
                            debug!(
                                "Transaction [{expected_hash}] on [{chain_id}] not yet mined (poll attempt {attempt}/{})",
                                options.max_poll_attempts
                            );
                        }
                        (None, Some(receipt)) => {
                            // Validate the receipt shape before handing it back to the module; the guest deserializes this string
                            // into a `TransactionReceipt`.
                            serde_json::from_value::<TransactionReceipt>(receipt.clone())
                                .map_err(BlockchainError::SerdeError)?;

                            return serde_json::to_string(&receipt)
                                .map_err(BlockchainError::SerdeError)
                                .map_err(ApiError::from);
                        }
                    }
                }
                // Transient failure (network error, 5xx, 429): the transaction
                // may still be mined, so keep polling.
                Err(ApiError::BlockchainError(e)) if e.is_retryable() => continue,
                Err(e) => return Err(e),
            }
        }

        Err(BlockchainError::Evm(EvmError::ConfirmationTimeout {
            transaction_hash: expected_hash,
            poll_attempts: options.max_poll_attempts,
        })
        .into())
    }
}

/// Validates the polling options for [`BlockchainClient::confirm_transaction`].
///
/// Returns an error if the options would either hammer the node pool or block a runtime thread
/// for an unbounded amount of time.
fn validate_poll_options(
    module: &PlaidModule,
    options: &ConfirmTransactionOptions,
) -> Result<(), ApiError> {
    if options.max_poll_attempts == 0 {
        warn!("Module [{module}] requested confirm_transaction with max_poll_attempts = 0");
        return Err(ApiError::BadRequest);
    }

    if options.poll_interval_ms < MIN_POLL_INTERVAL_MS {
        warn!(
            "Module [{module}] requested confirm_transaction with poll_interval_ms = {} (minimum {MIN_POLL_INTERVAL_MS})",
            options.poll_interval_ms
        );
        return Err(ApiError::BadRequest);
    }

    let confirmation_window_ms =
        u64::from(options.max_poll_attempts) * u64::from(options.poll_interval_ms);
    if confirmation_window_ms > MAX_CONFIRMATION_WINDOW_MS {
        warn!(
            "Module [{module}] requested confirm_transaction window of {confirmation_window_ms}ms (maximum {MAX_CONFIRMATION_WINDOW_MS}ms)"
        );
        return Err(ApiError::BadRequest);
    }

    Ok(())
}
