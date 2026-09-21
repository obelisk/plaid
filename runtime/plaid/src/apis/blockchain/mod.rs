use serde::Deserialize;
use std::sync::Arc;

use crate::apis::blockchain::{
    common::{BlockchainClient, ChainFamilyConfig},
    evm::Evm,
    solana::Solana,
};

pub mod common;
pub mod evm;
pub mod solana;

pub use common::BlockchainError;

/// Configuration for blockchain APIs
///
/// Currently only includes EVM configuration, but can be extended to non-EVM in the future
#[derive(Deserialize)]
pub struct BlockchainConfig {
    pub evm: Option<ChainFamilyConfig<Evm>>,
    pub solana: Option<ChainFamilyConfig<Solana>>,
}

/// Blockchain API clients
///
/// Clients are `Arc`'d so detached tasks (e.g. transaction-confirmation
/// pollers) can hold a cheap handle to them.
pub struct Blockchain {
    pub evm: Option<Arc<BlockchainClient<Evm>>>,
    pub solana: Option<Arc<BlockchainClient<Solana>>>,
}

impl Blockchain {
    /// Create a new Blockchain API client from the given configuration
    pub fn new(config: BlockchainConfig) -> Self {
        let evm = config.evm.map(|c| Arc::new(BlockchainClient::new(c)));
        let solana = config.solana.map(|c| Arc::new(BlockchainClient::new(c)));

        Self { evm, solana }
    }
}
