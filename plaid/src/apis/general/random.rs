use super::General;
use crate::{apis::ApiError, data::DelayedMessage, executor::Message};

use ring::rand::SecureRandom;

impl General {
    /// Generate randomness that can be used by running modules
    pub fn fetch_random_bytes(&self, num_bytes: u16) -> Result<Vec<u8>, ApiError> {
        let mut buf = vec![0; num_bytes as usize];
        match self.system_random.fill(&mut buf) {
            Ok(()) => Ok(buf),
            Err(_) => {
                error!("Failed to generate randomness!! This should be impossible.");
                return Err(ApiError::ImpossibleError);
            }
        }
    }
}
