use serde::Deserialize;

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
pub struct Blockchain {
    pub evm: Option<BlockchainClient<Evm>>,
    pub solana: Option<BlockchainClient<Solana>>,
}

impl Blockchain {
    /// Create a new Blockchain API client from the given configuration
    pub fn new(config: BlockchainConfig) -> Self {
        let evm = config.evm.map(BlockchainClient::new);
        let solana = config.solana.map(BlockchainClient::new);

        Self { evm, solana }
    }
}
