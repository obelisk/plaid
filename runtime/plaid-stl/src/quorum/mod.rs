use crate::PlaidFunctionError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VerifiedSignatureInfo {
    pub serial: String,
    pub fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub struct Proposal {
    pub data: Vec<u8>,
    pub description: String,
    pub signers: Vec<String>,
    pub required_signer_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct VerifiedProposalInfo {
    pub id: String,
    pub proposal: Proposal,
    pub raw_data: String,
    pub signers: Vec<VerifiedSignatureInfo>,
}

pub fn get_proposal_status(proposal_id: &str) -> Result<VerifiedProposalInfo, PlaidFunctionError> {
    extern "C" {
        fn quorum_proposal_status(
            proposal_id_buf: *const u8,
            proposal_id_buf_len: u32,
            return_buffer: *mut u8,
            return_buffer_length: u32,
        ) -> i32;
    }

    let id_bytes = proposal_id.as_bytes().to_vec();
    let mut return_buffer = vec![0; 1024 * 512];

    let res = unsafe {
        quorum_proposal_status(
            id_bytes.as_ptr(),
            id_bytes.len() as u32,
            return_buffer.as_mut_ptr(),
            return_buffer.len() as u32,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    // If the Plaid runtime doesn't lie to us, this should be fine.
    let serialized_prop = String::from_utf8(return_buffer).unwrap();

    // If we trust the Plaid runtime, this should never fail
    Ok(serde_json::from_str(&serialized_prop).unwrap())
}
