use crate::PlaidFunctionError;

pub fn fetch_random_bytes(num_bytes: u16) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        /// Fetch randomness from the host
        fn fetch_random_bytes(data: *mut u8, num_bytes: u16) -> i32;
    }

    let mut random_bytes = vec![0; num_bytes as usize];
    let ret = unsafe { fetch_random_bytes(random_bytes.as_mut_ptr(), num_bytes) };

    // This should always be the case but we check just in case
    if ret != num_bytes as i32 {
        Err(PlaidFunctionError::InternalApiError)
    } else {
        Ok(random_bytes)
    }
}
