use serde::Deserialize;

pub mod evm;

/// Configuration for blockchain APIs
///
/// Currently only includes EVM configuration, but can be extended to non-EVM in the future
#[derive(Deserialize)]
pub struct BlockchainConfig {
    pub evm: Option<evm::EvmConfig>,
}

/// Blockchain API clients
pub struct Blockchain {
    pub evm: Option<evm::EvmClient>,
}

impl Blockchain {
    /// Create a new Blockchain API client from the given configuration
    pub fn new(config: BlockchainConfig) -> Self {
        let evm = match config.evm {
            Some(evm_config) => Some(evm::EvmClient::new(evm_config)),
            _ => None,
        };

        Self { evm }
    }
}

#[derive(Debug)]
pub enum BlockchainError {
    EvmError(evm::EvmCallError),
}
