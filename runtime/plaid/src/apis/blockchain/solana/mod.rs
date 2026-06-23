mod utils;

use std::sync::Arc;

use plaid_stl::blockchain::solana::types::{
    Cluster, ClusterRequest, GetBlockRequest, GetFeeForMessageRequest,
    GetMinimumBalanceForRentExemptionRequest, GetMultipleAccountsRequest,
    GetProgramAccountsRequest, GetRecentPrioritizationFeesRequest, GetSignatureStatusesRequest,
    GetSignaturesForAddressRequest, GetTokenAccountsByOwnerRequest, GetTransactionRequest,
    PubkeyRequest, SendTransactionRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    apis::{
        blockchain::{
            common::{rpc::JsonRpcRequest, BlockchainClient, BlockchainError, ChainFamily},
            solana::utils::RpcMethods,
        },
        ApiError,
    },
    loader::PlaidModule,
};

/// SPL Token program id — the default filter for token-account queries.
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// The commitment (consistency) level applied to every Solana RPC call that accepts one.
///
/// Set once per Solana API in config.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Commitment {
    Processed,
    #[default]
    Confirmed,
    Finalized,
}

impl Commitment {
    /// `getBlock` and `getSignaturesForAddress` reject `processed` (a Solana RPC
    /// constraint), so floor `processed` to `confirmed` for those calls. `confirmed`
    /// and `finalized` pass through unchanged.
    fn block_safe(self) -> Self {
        match self {
            Self::Processed => Self::Confirmed,
            other => other,
        }
    }
}

/// Solana-specific configuration ([`ChainFamily::Options`]), read from the
/// `[apis."blockchain"."solana"]` table alongside the nodes/timeout/retries.
#[derive(Copy, Clone, Default, Deserialize)]
pub struct SolanaOptions {
    /// Commitment applied to all reads (and to preflight/simulation on writes).
    #[serde(default)]
    pub commitment: Commitment,
}

pub struct Solana;

impl ChainFamily for Solana {
    type Identifier = Cluster;
    type Options = SolanaOptions;
}

/// Solana-specific error conditions, carried by [`BlockchainError::Solana`].
///
/// Currently a placeholder: shared failure modes live on `BlockchainError` directly.
/// Add variants here if Solana-specific errors emerge.
#[derive(Debug)]
pub enum SolanaError {}

impl BlockchainClient<Solana> {
    /// Submits a fully-signed, b64 encoded transaction to the cluster's `sendTransaction` RPC.
    pub async fn send_signed_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<SendTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        // [base64Tx, { "encoding": "base64", "preflightCommitment": <configured> }]
        let params = (
            request.transaction,
            json!({ "encoding": "base64", "preflightCommitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::SendTransaction, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the lamport balance of an account.
    pub async fn get_balance(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<PubkeyRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.pubkey,
            json!({ "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetBalance, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns all information associated with an account.
    ///
    /// The Solana approach to reading on-chain state: rather than invoking a view function,
    /// you read the target account's raw data directly.
    pub async fn get_account_info(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<PubkeyRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        // base64 encoding handles account data of any size (base58, the default, caps out).
        let params = (
            request.pubkey,
            json!({ "encoding": "base64", "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetAccountInfo, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the slot that has reached the default commitment level.
    pub async fn get_slot(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<ClusterRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = Value::Array(vec![json!({ "commitment": self.options.commitment })]);
        let request = JsonRpcRequest::new(RpcMethods::GetSlot, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the latest blockhash, used as the recent blockhash when building transactions.
    pub async fn get_latest_blockhash(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<ClusterRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = Value::Array(vec![json!({ "commitment": self.options.commitment })]);
        let request = JsonRpcRequest::new(RpcMethods::GetLatestBlockhash, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the current cluster-wide transaction count.
    ///
    /// Note: unlike EVM's `getTransactionCount` (a per-address nonce), Solana's is the
    /// total number of transactions the cluster has processed. There is no per-account
    /// nonce on Solana; the closest per-address analog is `getSignaturesForAddress`.
    pub async fn get_transaction_count(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<ClusterRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = Value::Array(vec![json!({ "commitment": self.options.commitment })]);
        let request = JsonRpcRequest::new(RpcMethods::GetTransactionCount, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns details for a confirmed transaction by signature.
    ///
    /// The Solana analog of EVM's `getTransactionByHash`. The returned object also
    /// carries the execution `meta` (status, fee, logs), so it doubles as the receipt.
    pub async fn get_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.signature,
            json!({ "commitment": self.options.commitment, "maxSupportedTransactionVersion": 0 }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetTransaction, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the processing statuses of a batch of transaction signatures.
    ///
    /// The lightweight Solana analog of EVM's `getTransactionReceipt` — a quick
    /// "is it confirmed and did it succeed?" check without the full transaction body.
    pub async fn get_signature_statuses(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetSignatureStatusesRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.signatures,
            json!({ "searchTransactionHistory": request.search_transaction_history }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetSignatureStatuses, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns identity and transaction information about a confirmed block.
    ///
    /// The Solana analog of EVM's `getBlockByNumber`, keyed by slot rather than block number.
    pub async fn get_block(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetBlockRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            json!(request.slot),
            json!({
                "encoding": "json",
                "maxSupportedTransactionVersion": 0,
                "transactionDetails": "full",
                "rewards": false,
                "commitment": self.options.commitment.block_safe(),
            }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetBlock, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Reads multiple accounts in a single call.
    ///
    /// The batched form of [`Self::get_account_info`]; no EVM equivalent (EVM relies
    /// on multicall contracts to batch reads).
    pub async fn get_multiple_accounts(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetMultipleAccountsRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;
        let params = (
            request.pubkeys,
            json!({ "encoding": "base64", "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetMultipleAccounts, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Enumerates all accounts owned by a program.
    ///
    /// The fundamental Solana state query, with no EVM equivalent. Note this can be
    /// expensive and is rate-limited or disabled by many RPC providers.
    pub async fn get_program_accounts(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetProgramAccountsRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let mut config = serde_json::Map::new();
        config.insert("encoding".to_string(), json!("base64"));
        config.insert("commitment".to_string(), json!(self.options.commitment));

        // Translate the typed filters into Solana's `filters` array: an optional
        // `dataSize` plus one `memcmp` object per byte-match filter.
        let mut filters = Vec::new();
        if let Some(data_size) = request.filters.data_size {
            filters.push(json!({ "dataSize": data_size }));
        }
        for memcmp in request.filters.memcmp {
            let mut entry = serde_json::Map::new();
            entry.insert("offset".to_string(), json!(memcmp.offset));
            entry.insert("bytes".to_string(), json!(memcmp.bytes));
            if let Some(encoding) = memcmp.encoding {
                entry.insert("encoding".to_string(), json!(encoding));
            }
            filters.push(json!({ "memcmp": entry }));
        }
        if !filters.is_empty() {
            config.insert("filters".to_string(), Value::Array(filters));
        }

        if let Some(data_slice) = request.filters.data_slice {
            config.insert(
                "dataSlice".to_string(),
                json!({ "offset": data_slice.offset, "length": data_slice.length }),
            );
        }

        let params = (request.program_id, Value::Object(config));
        let request = JsonRpcRequest::new(RpcMethods::GetProgramAccounts, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Lists the SPL token accounts owned by a wallet, filtered by mint or token program.
    pub async fn get_token_accounts_by_owner(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetTokenAccountsByOwnerRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        // Prefer a mint filter when given; otherwise scope to a token program
        // (defaulting to SPL Token).
        let filter = match request.mint {
            Some(mint) => json!({ "mint": mint }),
            None if request.program_id.is_some() => json!({
                "programId": request
                    .program_id
                    .unwrap(),
            }),
            None => json!({"programId": SPL_TOKEN_PROGRAM_ID}),
        };
        let params = (
            request.owner,
            filter,
            json!({ "encoding": "base64", "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetTokenAccountsByOwner, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the token balance of an SPL token account.
    pub async fn get_token_account_balance(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<PubkeyRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.pubkey,
            json!({ "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetTokenAccountBalance, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the total supply of an SPL mint.
    pub async fn get_token_supply(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<PubkeyRequest>(params).map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.pubkey,
            json!({ "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetTokenSupply, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the lamports required for an account of a given size to be rent-exempt.
    ///
    /// Solana-specific (rent model); has no EVM equivalent.
    pub async fn get_minimum_balance_for_rent_exemption(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetMinimumBalanceForRentExemptionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            json!(request.data_length),
            json!({ "commitment": self.options.commitment }),
        );
        let request =
            JsonRpcRequest::new(RpcMethods::GetMinimumBalanceForRentExemption, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns the fee the cluster would charge for a serialized message.
    ///
    /// One of the Solana fee analogs to EVM's `estimate_gas`/`gas_price`.
    pub async fn get_fee_for_message(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetFeeForMessageRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.message,
            json!({ "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::GetFeeForMessage, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns recent prioritization (priority) fees, optionally scoped to accounts.
    ///
    /// The Solana analog of EVM's `gas_price` for setting priority fees.
    pub async fn get_recent_prioritization_fees(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetRecentPrioritizationFeesRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (!request.addresses.is_empty()).then_some(request.addresses);
        let request = JsonRpcRequest::new(RpcMethods::GetRecentPrioritizationFees, params);

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Simulates a signed transaction without submitting it.
    ///
    /// The closest Solana analog to EVM's `eth_call`: returns the would-be logs,
    /// compute units consumed, return data, and success/error without changing state.
    pub async fn simulate_transaction(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<SendTransactionRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let params = (
            request.transaction,
            json!({ "encoding": "base64", "commitment": self.options.commitment }),
        );
        let request = JsonRpcRequest::new(RpcMethods::SimulateTransaction, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }

    /// Returns signatures for transactions involving an address, most recent first.
    ///
    /// The per-address history analog (EVM has no direct equivalent over HTTP).
    pub async fn get_signatures_for_address(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<GetSignaturesForAddressRequest>(params)
            .map_err(BlockchainError::SerdeError)?;
        let cluster = request.cluster;

        let node_selector = self.get_node_selector(cluster)?;

        let mut config = serde_json::Map::new();
        config.insert(
            "commitment".to_string(),
            json!(self.options.commitment.block_safe()),
        );
        if let Some(limit) = request.limit {
            config.insert("limit".to_string(), json!(limit));
        }
        let params = (request.address, Value::Object(config));
        let request = JsonRpcRequest::new(RpcMethods::GetSignaturesForAddress, Some(params));

        self.execute_rpc_call(node_selector, cluster, request, module)
            .await
    }
}

// NOTE: These Solana integration tests are temporarily commented out, together with
// their dev-dependencies in Cargo.toml (surfpool-sdk, solana-transaction,
// solana-system-interface, bincode). surfpool-sdk pulls in the entire Solana validator
// dependency tree, which bloated runtime/Cargo.lock by ~14k lines. They are excluded for
// now to keep Cargo.lock small; restore this module and the dev-dependencies together to
// re-enable them.
/*
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::{json, Value};
    use solana_system_interface::instruction::transfer;
    use solana_transaction::Transaction;
    use surfpool_sdk::{Pubkey, Signer, Surfnet};
    use wasmer::{
        sys::{Cranelift, EngineBuilder},
        Module, Store,
    };

    use plaid_stl::blockchain::solana::types::Cluster;

    use crate::apis::blockchain::common::{
        node_selection::SelectionStrategy, BlockchainClient, ChainConfig, ChainFamilyConfig,
        NodeConfig,
    };
    use crate::loader::{LimitValue, PlaidModule};

    use super::{Commitment, Solana, SolanaOptions};

    /// Minimal blank module — just enough to satisfy the logging argument.
    fn test_module(name: &str) -> Arc<PlaidModule> {
        let store = Store::default();
        // stub wasm module, just enough to pass validation: \0ASM + version
        let wasm = &[0, 97, 115, 109, 1, 0, 0, 0];
        let engine = EngineBuilder::new(Cranelift::default());
        let module = Module::new(&store, wasm).unwrap();

        Arc::new(PlaidModule {
            name: name.to_string(),
            logtype: "test".to_string(),
            module,
            engine: engine.into(),
            computation_limit: 0,
            page_limit: 0,
            storage_current: Default::default(),
            storage_limit: LimitValue::Unlimited,
            accessory_data: Default::default(),
            secrets: Default::default(),
            persistent_response: Default::default(),
            test_mode: false,
        })
    }

    /// Build a `BlockchainClient<Solana>` with a single cluster pointed at `rpc_url`,
    /// using the default (confirmed) commitment.
    fn client_for(rpc_url: String) -> BlockchainClient<Solana> {
        client_for_with_commitment(rpc_url, Commitment::default())
    }

    /// Like [`client_for`], but with an explicit commitment level.
    fn client_for_with_commitment(
        rpc_url: String,
        commitment: Commitment,
    ) -> BlockchainClient<Solana> {
        let chains = HashMap::from([(
            Cluster::Mainnet,
            ChainConfig {
                nodes: vec![NodeConfig {
                    uri: rpc_url,
                    name: "surfpool".to_string(),
                    _tags: None,
                }],
                selection_strategy: SelectionStrategy::Random,
            },
        )]);

        BlockchainClient::new(ChainFamilyConfig {
            chains,
            timeout_millis: Duration::from_secs(10),
            max_retries: 3,
            options: SolanaOptions { commitment },
        })
    }

    /// Boot an embedded, offline (no mainnet fork) surfnet with a funded payer.
    async fn start_surfnet() -> Surfnet {
        Surfnet::builder()
            .offline(true)
            .airdrop_sol(1_000_000_000)
            .start()
            .await
            .unwrap()
    }

    /// A signed SOL transfer from the funded payer to a fresh recipient.
    fn signed_transfer(surfnet: &Surfnet) -> Transaction {
        let payer = surfnet.payer();
        let recipient = Pubkey::new_unique();
        let blockhash = surfnet.rpc_client().get_latest_blockhash().unwrap();
        // 1_000_000 lamports clears the 0-data rent-exempt minimum.
        let instruction = transfer(&payer.pubkey(), &recipient, 1_000_000);
        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        )
    }

    /// Materialize an initialized SPL mint at `mint` so token-account fixtures (and
    /// `getTokenSupply`) resolve. Packs the 82-byte SPL Token Mint layout by hand to
    /// avoid an `spl-token` dependency, mirroring surfpool-sdk's own `create_test_mint`.
    fn create_mint(surfnet: &Surfnet, mint: &Pubkey, supply: u64, decimals: u8) {
        // Layout: mint_authority COption<Pubkey> [0..36], supply u64 [36..44],
        // decimals u8 [44], is_initialized bool [45], freeze_authority COption<Pubkey> [46..82].
        // Both COption authorities left as None (zeroed tag + pubkey).
        let mut data = [0u8; 82];
        data[36..44].copy_from_slice(&supply.to_le_bytes());
        data[44] = decimals;
        data[45] = 1; // is_initialized
        let token_program = Pubkey::from_str(super::SPL_TOKEN_PROGRAM_ID).unwrap();
        // 1_461_600 lamports is the rent-exempt minimum for a Mint account.
        surfnet
            .cheatcodes()
            .set_account(mint, 1_461_600, &data, &token_program)
            .unwrap();
    }

    /// Parse a JSON-RPC response body, asserting it carries no error.
    fn parse_ok(response: &str) -> Value {
        let value: Value = serde_json::from_str(response).unwrap();
        assert!(
            value.get("error").is_none(),
            "JSON-RPC call returned an error: {response}"
        );
        value
    }

    fn params(body: Value) -> String {
        body.to_string()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_signed_transaction_succeeds() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let encoded = base64::encode(bincode::serialize(&tx).unwrap());

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .send_signed_transaction(
                &params(json!({ "cluster": "mainnet", "transaction": encoded })),
                test_module("solana_send_tx_test"),
            )
            .await
            .expect("send_signed_transaction should succeed");

        // The JSON-RPC result is the transaction signature.
        let value = parse_ok(&response);
        assert!(
            value["result"].is_string(),
            "expected a signature: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_balance_returns_funded_balance() {
        let surfnet = start_surfnet().await;
        let pubkey = surfnet.payer().pubkey().to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_balance(
                &params(json!({ "cluster": "mainnet", "pubkey": pubkey })),
                test_module("solana_get_balance_test"),
            )
            .await
            .expect("get_balance should succeed");

        let value = parse_ok(&response);
        let balance = value["result"]["value"]
            .as_u64()
            .unwrap_or_else(|| panic!("expected a numeric balance: {response}"));
        assert!(balance > 0, "expected a funded balance, got {balance}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_account_info_returns_account() {
        let surfnet = start_surfnet().await;
        let pubkey = surfnet.payer().pubkey().to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_account_info(
                &params(json!({ "cluster": "mainnet", "pubkey": pubkey })),
                test_module("solana_get_account_info_test"),
            )
            .await
            .expect("get_account_info should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"]["owner"].is_string(),
            "expected account info with an owner: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_slot_returns_number() {
        let surfnet = start_surfnet().await;

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_slot(
                &params(json!({ "cluster": "mainnet" })),
                test_module("solana_get_slot_test"),
            )
            .await
            .expect("get_slot should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"].is_u64(),
            "expected a slot number: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_latest_blockhash_returns_blockhash() {
        let surfnet = start_surfnet().await;

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_latest_blockhash(
                &params(json!({ "cluster": "mainnet" })),
                test_module("solana_get_latest_blockhash_test"),
            )
            .await
            .expect("get_latest_blockhash should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"]["blockhash"].is_string(),
            "expected a blockhash: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_transaction_count_returns_number() {
        let surfnet = start_surfnet().await;

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_transaction_count(
                &params(json!({ "cluster": "mainnet" })),
                test_module("solana_get_transaction_count_test"),
            )
            .await
            .expect("get_transaction_count should succeed");

        let value = parse_ok(&response);
        assert!(value["result"].is_u64(), "expected a count: {response}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_transaction_returns_confirmed_tx() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let signature = surfnet
            .rpc_client()
            .send_and_confirm_transaction(&tx)
            .unwrap()
            .to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_transaction(
                &params(json!({ "cluster": "mainnet", "signature": signature })),
                test_module("solana_get_transaction_test"),
            )
            .await
            .expect("get_transaction should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["meta"]["err"].is_null(),
            "expected a successful transaction: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_signature_statuses_returns_status() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let signature = surfnet
            .rpc_client()
            .send_and_confirm_transaction(&tx)
            .unwrap()
            .to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_signature_statuses(
                &params(json!({ "cluster": "mainnet", "signatures": [signature] })),
                test_module("solana_get_signature_statuses_test"),
            )
            .await
            .expect("get_signature_statuses should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"][0]["err"].is_null(),
            "expected a successful status: {response}"
        );
    }

    /// Exercises the non-default `search_transaction_history` path end to end.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_signature_statuses_with_history_search_returns_status() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let signature = surfnet
            .rpc_client()
            .send_and_confirm_transaction(&tx)
            .unwrap()
            .to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_signature_statuses(
                &params(json!({
                    "cluster": "mainnet",
                    "signatures": [signature],
                    "search_transaction_history": true,
                })),
                test_module("solana_get_signature_statuses_history_test"),
            )
            .await
            .expect("get_signature_statuses with history search should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"][0]["err"].is_null(),
            "expected a successful status: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_block_returns_block() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        surfnet
            .rpc_client()
            .send_and_confirm_transaction(&tx)
            .unwrap();
        let slot = surfnet.rpc_client().get_slot().unwrap();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_block(
                &params(json!({ "cluster": "mainnet", "slot": slot })),
                test_module("solana_get_block_test"),
            )
            .await
            .expect("get_block should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["blockhash"].is_string(),
            "expected a block with a blockhash: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_multiple_accounts_returns_accounts() {
        let surfnet = start_surfnet().await;
        let payer = surfnet.payer().pubkey().to_string();
        let missing = Pubkey::new_unique().to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_multiple_accounts(
                &params(json!({ "cluster": "mainnet", "pubkeys": [payer, missing] })),
                test_module("solana_get_multiple_accounts_test"),
            )
            .await
            .expect("get_multiple_accounts should succeed");

        let value = parse_ok(&response);
        let accounts = value["result"]["value"]
            .as_array()
            .unwrap_or_else(|| panic!("expected an array of accounts: {response}"));
        assert_eq!(accounts.len(), 2, "expected one entry per requested pubkey");
        assert!(
            !accounts[0].is_null(),
            "funded payer should exist: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_program_accounts_returns_array() {
        let surfnet = start_surfnet().await;
        // Materialize at least one token-program-owned account to query against.
        let owner = surfnet.payer().pubkey();
        let mint = Pubkey::new_unique();
        create_mint(&surfnet, &mint, 1_000_000_000, 0);
        surfnet
            .cheatcodes()
            .fund_token(&owner, &mint, 1_000, None)
            .unwrap();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_program_accounts(
                &params(json!({ "cluster": "mainnet", "program_id": super::SPL_TOKEN_PROGRAM_ID })),
                test_module("solana_get_program_accounts_test"),
            )
            .await
            .expect("get_program_accounts should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"].is_array(),
            "expected an array of program accounts: {response}"
        );
    }

    /// Proves the `dataSize` filter actually reaches the wire: a `82` filter (the Mint
    /// layout size) matches the mint but not the 165-byte token account, and a size no
    /// account has returns an empty set.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_program_accounts_data_size_filter_narrows_results() {
        let surfnet = start_surfnet().await;
        let owner = surfnet.payer().pubkey();
        let mint = Pubkey::new_unique();
        create_mint(&surfnet, &mint, 1_000_000_000, 0);
        surfnet
            .cheatcodes()
            .fund_token(&owner, &mint, 1_000, None)
            .unwrap();

        let client = client_for(surfnet.rpc_url().to_string());

        let response = client
            .get_program_accounts(
                &params(json!({
                    "cluster": "mainnet",
                    "program_id": super::SPL_TOKEN_PROGRAM_ID,
                    "filters": { "data_size": 82 },
                })),
                test_module("solana_get_program_accounts_data_size_test"),
            )
            .await
            .expect("get_program_accounts with a dataSize filter should succeed");
        let value = parse_ok(&response);
        let accounts = value["result"]
            .as_array()
            .unwrap_or_else(|| panic!("expected an array of program accounts: {response}"));
        assert!(
            accounts.iter().any(|a| a["pubkey"] == mint.to_string()),
            "dataSize=82 should match the mint account: {response}"
        );

        let response = client
            .get_program_accounts(
                &params(json!({
                    "cluster": "mainnet",
                    "program_id": super::SPL_TOKEN_PROGRAM_ID,
                    "filters": { "data_size": 999 },
                })),
                test_module("solana_get_program_accounts_no_match_test"),
            )
            .await
            .expect("get_program_accounts with a non-matching dataSize should succeed");
        let value = parse_ok(&response);
        let accounts = value["result"]
            .as_array()
            .unwrap_or_else(|| panic!("expected an array of program accounts: {response}"));
        assert!(
            accounts.is_empty(),
            "dataSize=999 should match no accounts: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_token_accounts_by_owner_returns_accounts() {
        let surfnet = start_surfnet().await;
        let owner = surfnet.payer().pubkey();
        let mint = Pubkey::new_unique();
        create_mint(&surfnet, &mint, 1_000_000_000, 0);
        surfnet
            .cheatcodes()
            .fund_token(&owner, &mint, 1_000, None)
            .unwrap();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_token_accounts_by_owner(
                &params(json!({
                    "cluster": "mainnet",
                    "owner": owner.to_string(),
                    "mint": mint.to_string(),
                })),
                test_module("solana_get_token_accounts_by_owner_test"),
            )
            .await
            .expect("get_token_accounts_by_owner should succeed");

        let value = parse_ok(&response);
        let accounts = value["result"]["value"]
            .as_array()
            .unwrap_or_else(|| panic!("expected an array of token accounts: {response}"));
        assert!(
            !accounts.is_empty(),
            "expected the funded token account: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_token_account_balance_returns_amount() {
        let surfnet = start_surfnet().await;
        let owner = surfnet.payer().pubkey();
        let mint = Pubkey::new_unique();
        create_mint(&surfnet, &mint, 1_000_000_000, 0);
        surfnet
            .cheatcodes()
            .fund_token(&owner, &mint, 1_000, None)
            .unwrap();
        let ata = surfnet
            .cheatcodes()
            .get_ata(&owner, &mint, None)
            .to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_token_account_balance(
                &params(json!({ "cluster": "mainnet", "pubkey": ata })),
                test_module("solana_get_token_account_balance_test"),
            )
            .await
            .expect("get_token_account_balance should succeed");

        let value = parse_ok(&response);
        assert_eq!(
            value["result"]["value"]["amount"], "1000",
            "expected the funded amount: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_token_supply_returns_supply() {
        let surfnet = start_surfnet().await;
        let owner = surfnet.payer().pubkey();
        let mint = Pubkey::new_unique();
        create_mint(&surfnet, &mint, 1_000_000_000, 0);
        surfnet
            .cheatcodes()
            .fund_token(&owner, &mint, 1_000, None)
            .unwrap();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_token_supply(
                &params(json!({ "cluster": "mainnet", "pubkey": mint.to_string() })),
                test_module("solana_get_token_supply_test"),
            )
            .await
            .expect("get_token_supply should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"]["amount"].is_string(),
            "expected a token supply amount: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_minimum_balance_for_rent_exemption_returns_number() {
        let surfnet = start_surfnet().await;

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_minimum_balance_for_rent_exemption(
                &params(json!({ "cluster": "mainnet", "data_length": 165 })),
                test_module("solana_get_min_balance_test"),
            )
            .await
            .expect("get_minimum_balance_for_rent_exemption should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"].is_u64(),
            "expected a rent-exemption amount: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_fee_for_message_returns_fee() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let message = base64::encode(bincode::serialize(&tx.message).unwrap());

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_fee_for_message(
                &params(json!({ "cluster": "mainnet", "message": message })),
                test_module("solana_get_fee_for_message_test"),
            )
            .await
            .expect("get_fee_for_message should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"].is_u64(),
            "expected a fee for the message: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_recent_prioritization_fees_returns_array() {
        let surfnet = start_surfnet().await;

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_recent_prioritization_fees(
                &params(json!({ "cluster": "mainnet" })),
                test_module("solana_get_recent_prio_fees_test"),
            )
            .await
            .expect("get_recent_prioritization_fees should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"].is_array(),
            "expected an array of recent fees: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn simulate_transaction_succeeds() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        let encoded = base64::encode(bincode::serialize(&tx).unwrap());

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .simulate_transaction(
                &params(json!({ "cluster": "mainnet", "transaction": encoded })),
                test_module("solana_simulate_tx_test"),
            )
            .await
            .expect("simulate_transaction should succeed");

        let value = parse_ok(&response);
        assert!(
            value["result"]["value"]["err"].is_null(),
            "expected a successful simulation: {response}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_signatures_for_address_returns_signatures() {
        let surfnet = start_surfnet().await;
        let tx = signed_transfer(&surfnet);
        surfnet
            .rpc_client()
            .send_and_confirm_transaction(&tx)
            .unwrap();
        let address = surfnet.payer().pubkey().to_string();

        let client = client_for(surfnet.rpc_url().to_string());
        let response = client
            .get_signatures_for_address(
                &params(json!({ "cluster": "mainnet", "address": address })),
                test_module("solana_get_signatures_for_address_test"),
            )
            .await
            .expect("get_signatures_for_address should succeed");

        let value = parse_ok(&response);
        let signatures = value["result"]
            .as_array()
            .unwrap_or_else(|| panic!("expected an array of signatures: {response}"));
        assert!(
            !signatures.is_empty(),
            "expected at least one signature: {response}"
        );
    }
}
*/
