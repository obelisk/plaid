mod utils;

use crate::{
    apis::{
        blockchain::{
            common::{
                rpc::JsonRpcRequest, BlockchainClient, BlockchainError, ChainFamily, NoOptions,
                PollOutcome, PollerKnobs, MAX_CONSECUTIVE_POLL_ERRORS,
            },
            evm::utils::RpcMethods,
        },
        ApiError,
    },
    functions::CallbackContext,
    loader::PlaidModule,
};
use plaid_stl::blockchain::evm::{
    parse_basic_rpc_response,
    types::{
        BasicRpcResponse, ChainId, ConfirmTransactionRequest, DetailedRpcResponse,
        EstimateGasRequest, EthCallRequest, GetAddressMetadataRequest, GetBlockRequest,
        GetFeeHistoryRequest, GetGasPriceRequest, GetLogsRequest, GetTransactionRequest,
        SendRawTransactionRequest, TransactionReceipt,
    },
};
use plaid_stl::blockchain::{ConfirmOutcome, ConfirmTransactionResult};
use plaid_stl::messages::{LogSource, SystemFunction};
use serde_json::{json, Number, Value};
use std::sync::Arc;
use tokio::time::Instant;

pub struct Evm;

impl ChainFamily for Evm {
    type Identifier = ChainId;
    type Options = NoOptions;
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

    /// Ask the runtime to broadcast a signed transaction and re-invoke the
    /// rule with a logback once the transaction reaches a terminal status.
    ///
    /// The broadcast happens synchronously (bounded by the RPC timeout); the
    /// receipt polling happens on a detached task so the guest is not blocked
    /// for the whole confirmation window.
    ///
    /// Takes `&Arc<Self>` so the spawned poller can hold an owned handle to
    /// the client.
    pub async fn confirm_transaction(
        self: &Arc<Self>,
        params: &str,
        module: Arc<PlaidModule>,
        callback: CallbackContext,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<ConfirmTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let chain_id = request.chain_id;

        // Never trust guest-provided knobs: clamp into safe bounds.
        let knobs = PollerKnobs::from_guest(request.poll_interval_ms, request.timeout_secs);

        // Broadcast synchronously so the rule learns the tx hash (and any
        // broadcast failure) immediately.
        let rpc_params = Value::Array(vec![Value::String(request.signed_tx.clone())]);
        let rpc_request = JsonRpcRequest::new(RpcMethods::SendRawTransaction, Some(rpc_params));

        let response = self
            .execute_rpc_call(chain_id, rpc_request, module.clone())
            .await?;

        let response = serde_json::from_str::<BasicRpcResponse>(&response)
            .map_err(BlockchainError::SerdeError)?;

        let transaction_hash = parse_basic_rpc_response(response)?;

        // Detached poller: polls the receipt until a terminal status or the
        // timeout elapses, then dispatches a logback to the rule. The timeout
        // starts at mempool acceptance, not task start.
        let deadline = Instant::now() + knobs.timeout;
        let poller_client = self.clone();
        let tx_hash = transaction_hash.clone();
        tokio::spawn(async move {
            let mut consecutive_errors = 0u32;
            let outcome = loop {
                if Instant::now() >= deadline {
                    break ConfirmOutcome::Error(format!(
                        "confirmation timed out after {}s",
                        knobs.timeout.as_secs()
                    ));
                }

                match poller_client
                    .poll_receipt(chain_id, &tx_hash, module.clone())
                    .await
                {
                    PollOutcome::Done(outcome) => break outcome,
                    PollOutcome::Error => {
                        consecutive_errors += 1;
                        if consecutive_errors >= MAX_CONSECUTIVE_POLL_ERRORS {
                            break ConfirmOutcome::Error(format!(
                                "receipt polling failed {consecutive_errors} times in a row; giving up"
                            ));
                        }
                    }
                    PollOutcome::Pending => {}
                }

                tokio::time::sleep(knobs.poll_interval).await;
            };

            let result = ConfirmTransactionResult {
                outcome,
                additional_data: request.additional_data,
                tx_id: tx_hash.clone(),
            };

            let payload = match serde_json::to_vec(&result) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Failed to serialize confirmation result for {tx_hash}: {e}");
                    return;
                }
            };

            // Route the logback to the rule's own log type so the executor
            // re-invokes it.
            if let Err(e) = callback.send_logback(
                module.logtype.clone(),
                payload,
                LogSource::System(SystemFunction::ConfirmTransaction),
            ) {
                error!(
                    "Failed to dispatch confirmation logback for {tx_hash} to {}: {e:?}",
                    module.name
                );
            }
        });

        Ok(transaction_hash)
    }

    /// Perform a single `eth_getTransactionReceipt` poll for `tx_hash` and
    /// classify the result.
    async fn poll_receipt(
        &self,
        chain_id: ChainId,
        tx_hash: &str,
        module: Arc<PlaidModule>,
    ) -> PollOutcome {
        let rpc_params = Value::Array(vec![Value::String(tx_hash.to_string())]);
        let rpc_request = JsonRpcRequest::new(RpcMethods::GetTransactionReceipt, Some(rpc_params));

        let response = match self.execute_rpc_call(chain_id, rpc_request, module).await {
            Ok(response) => response,
            Err(e) => {
                warn!("Receipt poll for {tx_hash} on {chain_id} errored: {e:?}.");
                return PollOutcome::Error;
            }
        };

        let parsed = match serde_json::from_str::<DetailedRpcResponse>(&response) {
            Ok(parsed) => parsed,
            Err(e) => {
                return PollOutcome::Done(ConfirmOutcome::Error(format!(
                    "failed to parse receipt response: {e}"
                )))
            }
        };

        let receipt = match parsed.result {
            // Terminal: the transaction has a receipt.
            Some(receipt) => receipt,
            // A pending transaction returns a null result with no error. Keep
            // polling.
            None => {
                if let Some(e) = parsed.error {
                    warn!(
                        "Receipt poll for {tx_hash} failed: {} {}. Retrying.",
                        e.code, e.message
                    );
                    return PollOutcome::Error;
                }
                return PollOutcome::Pending;
            }
        };

        match serde_json::from_value::<TransactionReceipt>(receipt) {
            Ok(receipt) => {
                info!(
                    "Transaction {tx_hash} confirmed with status {} on chain {chain_id}",
                    receipt.status
                );
                PollOutcome::Done(receipt.status.into())
            }
            Err(e) => PollOutcome::Done(ConfirmOutcome::Error(format!(
                "failed to parse receipt: {e}"
            ))),
        }
    }
}
