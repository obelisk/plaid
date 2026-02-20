use std::sync::Arc;

use fastbloom::BloomFilter as FastBloomFilter;
use plaid_stl::bloom_filter::{BloomFilterInternals, BloomFilterPayload};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;

use crate::{apis::ApiError, loader::PlaidModule};

#[derive(Deserialize)]
pub struct BloomFilterConfig {}

pub struct BloomFilter {}

impl BloomFilter {
    pub fn new(_config: BloomFilterConfig) -> Self {
        Self {}
    }

    /// Build a bloom filter with the given parameters and items, returning the internals of the bloom filter as a JSON string.
    pub async fn build_with_items(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        info!("Building Bloom filter on behalf of module [{module}]");

        let payload: BloomFilterPayload = serde_json::from_str(params).map_err(|e| {
            ApiError::BloomFilterError(format!("Failed to parse bloom filter params: {}", e))
        })?;
        let mut seed = [0u8; 16];
        SystemRandom::new().fill(&mut seed).map_err(|_| {
            ApiError::BloomFilterError("Failed to generate seed for bloom filter".to_string())
        })?;
        let seed_u128 = u128::from_le_bytes(seed);

        let mut filter = FastBloomFilter::with_false_pos(payload.params.false_positive_rate)
            .seed(&seed_u128)
            .expected_items(payload.params.expected_num_items);

        filter.insert_all(&payload.items);

        let internals = BloomFilterInternals {
            bytes: filter.iter().collect(),
            seed: seed_u128,
            num_hashes: filter.num_hashes(),
        };
        let internals = serde_json::to_string(&internals).map_err(|e| {
            ApiError::BloomFilterError(format!("Failed to serialize bloom filter internals: {}", e))
        })?;

        Ok(internals)
    }
}
