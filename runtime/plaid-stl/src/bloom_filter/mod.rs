use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

/// Parameters for building a bloom filter, which can be used to efficiently test set membership with a tunable false positive rate.
#[derive(Serialize, Deserialize, Clone)]
pub struct BloomFilterParams {
    pub expected_num_items: usize,
    pub false_positive_rate: f64,
}

/// Payload for building a bloom filter, which includes the parameters as well as the items to insert into the filter.
#[derive(Serialize, Deserialize)]
pub struct BloomFilterPayload {
    pub params: BloomFilterParams,
    pub items: Vec<String>,
}

/// Internals of a bloom filter, which include the bytes representing the filter, the seed used for hashing, and the number of hash functions.
/// This can be used to reconstruct the bloom filter for later use.
#[derive(Serialize, Deserialize)]
pub struct BloomFilterInternals {
    pub bytes: Vec<u64>,
    pub seed: u128,
    pub num_hashes: u32,
}

/// Build a bloom filter with the given parameters and items, returning the internals of the bloom filter.
///
/// Args:
/// - `params`: Parameters for building the bloom filter, including expected number of items and false positive rate.
/// - `items`: The items to insert into the bloom filter.
///
/// Returns:
/// - On success, returns the internals of the bloom filter, which can be used to reconstruct the filter for later use.
/// - On failure, returns a `PlaidFunctionError` indicating what went wrong.
pub fn build_with_items(
    params: &BloomFilterParams,
    items: &[impl Display],
) -> Result<BloomFilterInternals, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(bloom_filter, build_with_items);
    }

    let payload = BloomFilterPayload {
        params: params.clone(),
        items: items.iter().map(|s| s.to_string()).collect(),
    };

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        bloom_filter_build_with_items(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    Ok(serde_json::from_str(&String::from_utf8(return_buffer).unwrap()).unwrap())
}
