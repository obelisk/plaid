use crate::PlaidFunctionError;

pub enum CacheError {
    BufferSizeMismatch,
}

pub fn insert(key: &str, value: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        /// Send a request to store this data in whatever persistence system Plaid has configured.
        /// There may not be one which is not visible to services. This will be addressed in a
        /// future update.
        fn cache_insert(
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;

        fn cache_get(key: *const u8, key_len: usize, data: *const u8, data_len: usize) -> i32;
    }

    let key_bytes = key.as_bytes().to_vec();
    let value_bytes = value.as_bytes().to_vec();

    let buffer_size =
        unsafe { cache_get(key_bytes.as_ptr(), key_bytes.len(), vec![].as_mut_ptr(), 0) };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }

    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        cache_insert(
            key_bytes.as_ptr(),
            key_bytes.len(),
            value_bytes.as_ptr(),
            value_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(String::from_utf8(data_buffer).unwrap())
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn get(key: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        fn cache_get(key: *const u8, key_len: usize, data: *const u8, data_len: usize) -> i32;
    }

    let key_bytes = key.as_bytes().to_vec();

    let buffer_size =
        unsafe { cache_get(key_bytes.as_ptr(), key_bytes.len(), vec![].as_mut_ptr(), 0) };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        cache_get(
            key_bytes.as_ptr(),
            key_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        Ok(String::from_utf8(data_buffer).unwrap())
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}
