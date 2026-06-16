pub mod node_selection;
pub mod rpc;

use crate::{
    apis::{
        blockchain::{
            common::{
                node_selection::{
                    selection_strategy_deserializer, NodeSelector, SelectionStrategy,
                },
                rpc::JsonRpcRequest,
            },
            evm::EvmError,
            solana::SolanaError,
        },
        ApiError,
    },
    loader::PlaidModule,
    parse_duration,
};
use http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode};
use reqwest::Client;
use serde::{de, Deserialize, Serialize};
use std::{
    collections::HashMap, fmt::Display, hash::Hash, str::FromStr, sync::Arc, time::Duration,
};

pub trait ChainFamily {
    /// The per-chain identifier used as the TOML map key (e.g. EVM `ChainId`, Solana `Cluster`).
    type Identifier: FromStr<Err: Display> + Eq + Hash + Display + Copy;
    /// Family-specific configuration that lives alongside `chains`/`timeout_millis`/etc.
    /// (e.g. Solana's commitment level). Families with no extra options use [`NoOptions`].
    type Options: serde::de::DeserializeOwned + Default + Copy;
}

/// Placeholder [`ChainFamily::Options`] for families that have no family-specific config.
#[derive(Default, Clone, Copy, Deserialize)]
pub struct NoOptions {}

/// Errors common to all blockchain families, plus a chain-specific variant per family.
#[derive(Debug)]
pub enum BlockchainError {
    SerdeError(serde_json::Error),
    NetworkError(reqwest::Error),
    CallFailed {
        status: StatusCode,
        message: String,
    },
    /// No configured nodes for the requested chain identifier.
    NoNodes {
        // ChainId for EVM, cluster identifier for Solana
        identifier: String,
    },
    AllNodesFailed,
    Evm(EvmError),
    Solana(SolanaError),
}

impl BlockchainError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::CallFailed { status, .. } => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            Self::NetworkError(_) => true,
            _ => false,
        }
    }
}

pub struct BlockchainClient<C: ChainFamily> {
    pub(crate) node_selector: HashMap<C::Identifier, NodeSelector>,
    pub(crate) client: Client,
    pub(crate) max_retries: u8,
    pub(crate) options: C::Options,
}

/// Default timeout duration for client requests
pub(crate) fn default_timeout() -> Duration {
    Duration::from_millis(3000)
}

/// Default maximum number of retries for client requests
pub(crate) fn default_max_retries() -> u8 {
    3
}

#[derive(Deserialize, Clone)]
pub struct NodeConfig {
    /// The URIs of the RPC nodes to connect to
    pub(crate) uri: String,
    /// A human-readable name for the node
    pub(crate) name: String,
    /// Optional tags for the node
    #[serde(rename = "tags")]
    pub(crate) _tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
// `C` is a marker type and is never itself deserialized (only `C::Identifier` and
// `C::Options` are), so constrain the derive's synthesized bound to just `C::Options`.
#[serde(bound(deserialize = "C::Options: serde::Deserialize<'de>"))]
pub struct ChainFamilyConfig<C: ChainFamily> {
    /// Per-chain configuration, keyed by the family's identifier.
    /// TOML keys arrive as strings and are parsed via `Identifier: FromStr`.
    #[serde(deserialize_with = "deserialize_chains")]
    pub chains: HashMap<C::Identifier, ChainConfig>,
    /// Timeout duration for client requests in milliseconds
    #[serde(default = "default_timeout")]
    #[serde(deserialize_with = "parse_duration")]
    pub timeout_millis: Duration,
    /// The maximum number of retries for client requests
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,
    /// Family-specific options (e.g. Solana commitment), read from the same table.
    #[serde(flatten, default)]
    pub options: C::Options,
}

/// Deserializes the `chains` map by reading string TOML keys and parsing each
/// into the family's identifier via `FromStr`. `K` is inferred as `C::Identifier`
/// from the field type at the call site.
fn deserialize_chains<'de, D, K>(deserializer: D) -> Result<HashMap<K, ChainConfig>, D::Error>
where
    D: de::Deserializer<'de>,
    K: FromStr<Err: Display> + Eq + Hash,
{
    HashMap::<String, ChainConfig>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, config)| Ok((key.parse().map_err(de::Error::custom)?, config)))
        .collect()
}

#[derive(Deserialize)]
pub struct ChainConfig {
    /// The list of nodes for this chain
    pub(crate) nodes: Vec<NodeConfig>,
    /// The selection strategy for this chain
    #[serde(deserialize_with = "selection_strategy_deserializer")]
    pub(crate) selection_strategy: SelectionStrategy,
}

impl<C: ChainFamily> BlockchainClient<C> {
    pub fn new(config: ChainFamilyConfig<C>) -> Self {
        let mut default_headers = HeaderMap::new();

        let content_type_value = HeaderValue::from_static("application/json");
        default_headers.insert(CONTENT_TYPE, content_type_value);

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .timeout(config.timeout_millis)
            .build()
            .expect("Failed to build blockchain reqwest client");

        let node_selector = config
            .chains
            .into_iter()
            .map(|(chain_id, config)| {
                let selector = NodeSelector::new(config.nodes, config.selection_strategy);
                (chain_id, selector)
            })
            .collect();

        Self {
            client,
            node_selector,
            max_retries: config.max_retries,
            options: config.options,
        }
    }

    /// Look up the node selector for a chain identifier, erroring if none is configured.
    pub(crate) fn get_node_selector(
        &self,
        identifier: C::Identifier,
    ) -> Result<&NodeSelector, ApiError> {
        self.node_selector.get(&identifier).ok_or_else(|| {
            BlockchainError::NoNodes {
                identifier: identifier.to_string(),
            }
            .into()
        })
    }

    /// Execute a JSON-RPC call with automatic retry and failure handling.
    ///
    /// This handles:
    /// - Retry logic up to `max_retries` attempts
    /// - Automatic failure marking to deprioritize bad nodes
    ///
    /// It is generic over the method type `M`, so each chain family reuses this
    /// loop with its own set of RPC method names.
    pub(crate) async fn execute_rpc_call<M: Serialize + Display>(
        &self,
        selector: &NodeSelector,
        identifier: C::Identifier,
        json_rpc_request: JsonRpcRequest<'_, M>,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let mut last_error = BlockchainError::AllNodesFailed;

        debug!(
            "Module [{module}] is attempting to call [{}] on chain [{}]",
            json_rpc_request.method, identifier,
        );
        for attempt in 1..=self.max_retries {
            // Get the next node to try
            let Some(node) = selector.select_node() else {
                return Err(BlockchainError::NoNodes {
                    identifier: identifier.to_string(),
                }
                .into());
            };

            trace!(
                "Attempt {attempt}/{} for RPC call [{}] using node [{}] on behalf of module [{module}]",
                self.max_retries,
                json_rpc_request.method,
                node.name
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
        }

        Err(last_error.into())
    }
}
