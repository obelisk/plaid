mod selector;
mod utils;

use crate::{
    apis::{
        blockchain::evm::{
            selector::{NodeSelector, SelectionStrategy},
            utils::{JsonRpcRequest, RpcMethods},
        },
        ApiError,
    },
    loader::PlaidModule,
    parse_duration,
};
use http::StatusCode;
use plaid_stl::blockchain::evm::types::{
    ChainId, EstimateGasRequest, EthCallRequest, GetAddressMetadataRequest, GetGasPriceRequest,
    GetTransactionRequest, SendRawTransactionRequest,
};
use reqwest::Client;
use serde::{de, Deserialize};
use serde_json::{json, Value};
use serde_with::{serde_as, DisplayFromStr};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

#[derive(Debug)]
pub enum EvmCallError {
    SerdeError(serde_json::Error),
    NetworkError(reqwest::Error),
    CallFailed { status: StatusCode, message: String },
    NoNodesForChain { id: u64 },
    BadRequest(serde_json::Error),
    AllNodesFailed,
    HttpError { status: u16, message: String },
    JsonRpcError { code: i64, message: String },
    InvalidResponse(String),
}

impl EvmCallError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::CallFailed { status, .. } => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            _ => false,
        }
    }
}

#[serde_as]
#[derive(Deserialize)]
pub struct EvmConfig {
    // Keys are strings in TOML, parsed to ChainId via DisplayFromStr
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    chains: HashMap<ChainId, ChainConfig>,
    /// Timeout duration for EVM client requests in milliseconds
    #[serde(default = "default_timeout")]
    #[serde(deserialize_with = "parse_duration")]
    timeout_millis: Duration,
    /// The maximum number of retries for EVM client requests
    #[serde(default = "default_max_retries")]
    max_retries: u8,
}

#[derive(Deserialize)]
pub struct ChainConfig {
    /// The list of nodes for this chain
    pub nodes: Vec<NodeConfig>,
    /// The selection strategy for this chain
    #[serde(deserialize_with = "selection_strategy_deserializer")]
    pub selection_strategy: SelectionStrategy,
}

/// Deserialized for a webhook's response mode
fn selection_strategy_deserializer<'de, D>(deserializer: D) -> Result<SelectionStrategy, D::Error>
where
    D: de::Deserializer<'de>,
{
    let strategy = String::deserialize(deserializer)?;
    match strategy.to_lowercase().as_str() {
        "roundrobin" => Ok(SelectionStrategy::RoundRobin {
            current_index: AtomicUsize::new(0),
        }),
        "random" => Ok(SelectionStrategy::Random),
        _ => Err(de::Error::custom(format!(
            "Unknown selection strategy: {strategy}",
        ))),
    }
}

#[derive(Deserialize, Clone)]
pub struct NodeConfig {
    /// The URIs of the RPC nodes to connect to
    pub uri: String,
    /// A human-readable name for the node
    pub name: String,
    /// Optional tags for the node
    pub tags: Option<Vec<String>>,
}

/// Default timeout duration for EVM client requests
fn default_timeout() -> Duration {
    Duration::from_millis(3000)
}

/// Default maximum number of retries for EVM client requests
fn default_max_retries() -> u8 {
    3
}

pub struct EvmClient {
    node_selector: HashMap<ChainId, NodeSelector>,
    client: Client,
    max_retries: u8,
}

impl EvmClient {
    pub fn new(config: EvmConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout_millis)
            .build()
            .expect("Failed to build EVM reqwest client");

        let selector = config
            .chains
            .into_iter()
            .map(|(chain_id, config)| {
                let selector =
                    NodeSelector::new(chain_id.get(), config.nodes, config.selection_strategy);
                (chain_id, selector)
            })
            .collect();

        Self {
            client,
            node_selector: selector,
            max_retries: config.max_retries,
        }
    }

    /// Returns the information about a transaction requested by transaction hash.
    pub async fn get_transaction_by_hash(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetTransactionRequest>(params)
            .map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionByHash, Some(params));

        self.execute_rpc_call(selector, request, module).await
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
            .map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let params = Value::Array(vec![Value::String(request.hash)]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionReceipt, Some(params));

        self.execute_rpc_call(selector, request, module).await
    }

    /// Creates new message call transaction or a contract creation for signed transactions.
    pub async fn send_raw_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<SendRawTransactionRequest>(params)
            .map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let params = Value::Array(vec![Value::String(request.signed_tx)]);
        let request = JsonRpcRequest::new(RpcMethods::SendRawTransaction, Some(params));

        self.execute_rpc_call(selector, request, module).await
    }

    /// Returns the number of transactions sent from an address.
    pub async fn get_transaction_count(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetAddressMetadataRequest>(params)
            .map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionCount, Some(params));

        self.execute_rpc_call(selector, request, module).await
    }

    /// Returns the balance of the account at a given address.
    pub async fn get_balance(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetAddressMetadataRequest>(params)
            .map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let params = Value::Array(vec![
            Value::String(request.address),
            Value::String(request.block_tag.to_string()),
        ]);
        let request = JsonRpcRequest::new(RpcMethods::GetBalance, Some(params));

        self.execute_rpc_call(selector, request, module).await
    }

    /// Generates and returns an estimate of how much gas is necessary to allow the transaction to complete.
    pub async fn estimate_gas(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<EstimateGasRequest>(params).map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let mut object = serde_json::Map::new();
        object.insert("from".to_string(), Value::String(request.from.to_string()));

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

        self.execute_rpc_call(selector, request, module).await
    }

    /// Executes a new message call immediately without creating a transaction on the blockchain.
    /// Often used for executing read-only smart contract functions, for example the balanceOf for an ERC-20 contract.
    pub async fn eth_call(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<EthCallRequest>(params).map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let object = json!({ "to": request.to, "data": request.data });
        let params = Value::Array(vec![object, Value::String(request.block_tag.to_string())]);
        let request = JsonRpcRequest::new(RpcMethods::Call, Some(params));

        self.execute_rpc_call(selector, request, module).await
    }

    /// Returns an estimate of the current price per gas in wei. For example, the Besu client examines the last 100 blocks and returns the median gas unit price by default.
    pub async fn gas_price(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetGasPriceRequest>(params).map_err(EvmCallError::SerdeError)?;

        let selector = self.get_node_selector(request.chain_id)?;

        let request = JsonRpcRequest::new(RpcMethods::GasPrice, None);

        self.execute_rpc_call(selector, request, module).await
    }

    fn get_node_selector(&self, chain_id: ChainId) -> Result<&NodeSelector, ApiError> {
        self.node_selector
            .get(&chain_id)
            .ok_or(EvmCallError::NoNodesForChain { id: chain_id.get() }.into())
    }

    /// Execute an EVM RPC call with automatic retry and failure handling
    ///
    /// This method handles:
    /// - Node selection from the configured selector
    /// - Retry logic up to max_retries attempts
    /// - Automatic failure marking to deprioritize bad nodes
    /// - Uses existing JsonRpcRequest utilities
    async fn execute_rpc_call<'a>(
        &self,
        selector: &NodeSelector,
        json_rpc_request: JsonRpcRequest<'a>,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let mut last_error = EvmCallError::AllNodesFailed;

        for attempt in 0..=self.max_retries {
            // Get the next node to try
            let Some(node) = selector.select_node() else {
                return Err(EvmCallError::NoNodesForChain { id: selector.id }.into());
            };
            debug!(
                "Module [{module}] is attempting to call [{}] on node [{}] on chain with ID [{}] (attempt {attempt}/{})",
                json_rpc_request.method,
                node.name,
                selector.id,
                self.max_retries
            );

            // Make the RPC call using the existing utility
            match json_rpc_request.execute(&self.client, &node.uri).await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(e) => {
                    // If the error is not retryable, return immediately
                    if !e.is_retryable() {
                        return Err(e.into());
                    }

                    // Call failed, mark node as failed and try next
                    selector.mark_current_node_failed();
                    last_error = e;
                }
            }

            if attempt == self.max_retries {
                break;
            }
        }

        Err(last_error.into())
    }
}
