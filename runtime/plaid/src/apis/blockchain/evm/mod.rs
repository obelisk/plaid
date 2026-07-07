mod utils;

use crate::{
    apis::{
        blockchain::{
            common::{
                rpc::JsonRpcRequest, BlockchainClient, BlockchainError, ChainFamily, NoOptions,
            },
            evm::utils::RpcMethods,
        },
        ApiError,
    },
    loader::PlaidModule,
};
use plaid_stl::blockchain::evm::types::{
    ChainId, EstimateGasRequest, EthCallRequest, GetAddressMetadataRequest, GetBlockRequest,
    GetFeeHistoryRequest, GetGasPriceRequest, GetLogsRequest, GetTransactionRequest,
    SendRawTransactionRequest,
};
use serde_json::{json, Number, Value};
use std::sync::Arc;

pub struct Evm;

impl ChainFamily for Evm {
    type Identifier = ChainId;
    type Options = NoOptions;
}

/// EVM-specific error conditions, carried by [`BlockchainError::Evm`].
///
/// Currently a placeholder: every failure mode the EVM client hits today is
/// common to all supported chains and lives on `BlockchainError` directly.
#[derive(Debug)]
pub enum EvmError {}

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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionByHash, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionReceipt, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = Value::Array(vec![Value::String(request.signed_tx)]);
        let request = JsonRpcRequest::new(RpcMethods::SendRawTransaction, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionCount, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetBalance, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

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

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let object = json!({ "to": request.to, "data": request.data });
        let params = Value::Array(vec![object, Value::String(request.block_tag.to_string())]);
        let request = JsonRpcRequest::new(RpcMethods::Call, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let request = JsonRpcRequest::<_, ()>::new(RpcMethods::GasPrice, None);

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

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

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let params = serde_json::Value::Array(vec![
            Value::String(request.block_tag.to_string()),
            Value::Bool(request.hydrated_transactions),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetBlock, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
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

        let node_selector = self.get_node_selector(chain_id)?;

        let mut params = vec![
            Value::String(format!("0x{:x}", request.block_count)),
            Value::String(request.block_tag.to_string()),
        ];
        if let Some(percentiles) = request.reward_percentiles {
            let percentiles = percentiles
                .into_iter()
                .map(|perc| Value::Number(Number::from(perc as u16)))
                .collect::<Vec<_>>();

            params.push(Value::Array(percentiles));
        }

        let params = Value::Array(params);
        let request = JsonRpcRequest::new(RpcMethods::GetFeeHistory, Some(params));

        self.execute_rpc_call(node_selector, chain_id, request, module)
            .await
    }
}
