use crate::apis::ApiError;

pub use quorum_agent::QuorumSettings as QuorumConfig;
use quorum_agent::{Quorum as QuorumServerAPI, VerifiedProposalInfo, ProposalStatus};

pub struct Quorum {
    agent: QuorumServerAPI,
}

#[derive(Debug)]
pub enum QuorumError {
    CouldNotReachQuorumServer,
    ValidationError,
    MismatchedProposalId,
}

impl Quorum {
    pub fn new(config: QuorumConfig) -> Self {
        let agent = QuorumServerAPI::create(config);

        Self {
            agent,
        }
    }

    pub async fn proposal_status(&self, id: &str, _: &str) -> Result<String, ApiError> {
        let result: ProposalStatus = self.agent.check_proposal_status(&id).await.map_err(|e| {
            error!("Quorum API encountered an error: {}", e.to_string());
            ApiError::QuorumError(QuorumError::CouldNotReachQuorumServer)
        })?.into();

        let vpi: VerifiedProposalInfo = result.try_into().map_err(|e| {
            error!("Quorum API returned data that failed to validate: {:?}", e);
            ApiError::QuorumError(QuorumError::ValidationError)
        })?;

        // Verify the proposal we got back from the server is the one we asked for
        if id != vpi.id {
            return Err(ApiError::QuorumError(QuorumError::MismatchedProposalId))
        }

        let data = serde_json::to_string(&vpi).unwrap();
        
        return Ok(data)
    }
}
