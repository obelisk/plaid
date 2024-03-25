#[cfg(feature = "quorum")]
pub mod quorum;
#[cfg(feature = "quorum")]
pub use quorum::*;


/// If quorum is not enabled, stub out the structures
#[cfg(not(feature = "quorum"))]
pub struct Quorum {}

#[cfg(not(feature = "quorum"))]
impl Quorum {
    pub async fn proposal_status(&self, _: &str, _: &str) -> Result<String, super::ApiError> {
        Err(super::ApiError::ConfigurationError("Quorum is not enabled".to_string()))
    }
}

#[cfg(not(feature = "quorum"))]
#[derive(serde::Deserialize)]
pub struct QuorumConfig {}

#[cfg(not(feature = "quorum"))]
#[derive(Debug)]
pub enum QuorumError {}