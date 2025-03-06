use std::fmt::Display;

use crate::PlaidFunctionError;

pub enum StorageError {
    BufferSizeMismatch,
}

pub fn insert(key: &str, value: &[u8]) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        /// Send a request to store this data in whatever persistence system Plaid has configured.
        /// There may not be one which is not visible to services. This will be addressed in a
        /// future update.
        fn storage_insert(
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> u32;

        fn storage_get(key: *const u8, key_len: usize, data: *const u8, data_len: usize) -> i32;
    }

    let key_bytes = key.as_bytes().to_vec();

    let buffer_size =
        unsafe { storage_get(key_bytes.as_ptr(), key_bytes.len(), vec![].as_mut_ptr(), 0) };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }

    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_insert(
            key_bytes.as_ptr(),
            key_bytes.len(),
            value.as_ptr(),
            value.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size as u32 {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn insert_shared(
    namespace: &str,
    key: &str,
    value: &[u8],
) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        /// Send a request to store this data in whatever persistence system Plaid has configured.
        /// There may not be one which is not visible to services. This will be addressed in a
        /// future update.
        fn storage_insert_shared(
            namespace: *const u8,
            namespace_len: usize,
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> u32;

        fn storage_get_shared(
            namespace: *const u8,
            namespace_len: usize,
            key: *const u8,
            key_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;
    }

    let namespace_bytes = namespace.as_bytes().to_vec();
    let key_bytes = key.as_bytes().to_vec();

    let buffer_size = unsafe {
        storage_get_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            vec![].as_mut_ptr(),
            0,
        )
    };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }

    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_insert_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            value.as_ptr(),
            value.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size as u32 {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn get(key: &str) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_get(key: *const u8, key_len: usize, data: *const u8, data_len: usize) -> i32;
    }

    let key_bytes = key.as_bytes().to_vec();

    let buffer_size =
        unsafe { storage_get(key_bytes.as_ptr(), key_bytes.len(), vec![].as_mut_ptr(), 0) };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_get(
            key_bytes.as_ptr(),
            key_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn get_shared(namespace: &str, key: &str) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_get_shared(
            namespace: *const u8,
            namespace_len: usize,
            key: *const u8,
            key_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;
    }

    let namespace_bytes = namespace.as_bytes().to_vec();
    let key_bytes = key.as_bytes().to_vec();

    let buffer_size = unsafe {
        storage_get_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            vec![].as_mut_ptr(),
            0,
        )
    };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_get_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

/// List all the keys set by this rule in the runtime. An optional
/// prefix can be provided so that only a subset of keys is returned
pub fn list_keys(prefix: Option<impl Display>) -> Result<Vec<String>, PlaidFunctionError> {
    extern "C" {
        fn storage_list_keys(
            prefix: *const u8,
            prefix_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;
    }

    let prefix = match prefix {
        Some(p) => p.to_string(),
        None => String::new(),
    };

    let prefix_bytes = prefix.as_bytes().to_vec();

    let buffer_size = unsafe {
        storage_list_keys(
            prefix_bytes.as_ptr(),
            prefix_bytes.len(),
            vec![].as_mut_ptr(),
            0,
        )
    };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_list_keys(
            prefix_bytes.as_ptr(),
            prefix_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        serde_json::from_slice(&data_buffer).map_err(|_| PlaidFunctionError::InternalApiError)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn delete(key: &str) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_delete(key: *const u8, key_len: usize, data: *const u8, data_len: usize) -> i32;
    }

    let key_bytes = key.as_bytes().to_vec();

    let buffer_size =
        unsafe { storage_delete(key_bytes.as_ptr(), key_bytes.len(), vec![].as_mut_ptr(), 0) };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_delete(
            key_bytes.as_ptr(),
            key_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

pub fn delete_shared(namespace: &str, key: &str) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_delete_shared(
            namespace: *const u8,
            namespace_len: usize,
            key: *const u8,
            key_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;
    }

    let namespace_bytes = namespace.as_bytes().to_vec();
    let key_bytes = key.as_bytes().to_vec();

    let buffer_size = unsafe {
        storage_delete_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            vec![].as_mut_ptr(),
            0,
        )
    };

    if buffer_size < 0 {
        return Err(buffer_size.into());
    }
    let mut data_buffer = vec![0; buffer_size as usize];
    let copied_size = unsafe {
        storage_delete_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size as usize,
        )
    };

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}
