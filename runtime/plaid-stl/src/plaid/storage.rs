use std::fmt::Display;

use crate::PlaidFunctionError;

pub enum StorageError {
    BufferSizeMismatch,
}

/// Store `value` at `key` in this rule's namespace.
///
/// Returns the **previous** value at `key`, not the value that was written. If the key is new,
/// returns an empty vector.
pub fn insert(key: &str, value: &[u8]) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_insert(
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;

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

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

/// Store `value` at `key` in a shared namespace.
///
/// Returns the **previous** value at `key`, not the value that was written. If the key is new,
/// returns an empty vector.
///
/// The rule must have read-write access to `namespace` in Plaid's configuration.
pub fn insert_shared(
    namespace: &str,
    key: &str,
    value: &[u8],
) -> Result<Vec<u8>, PlaidFunctionError> {
    extern "C" {
        fn storage_insert_shared(
            namespace: *const u8,
            namespace_len: usize,
            key: *const u8,
            key_len: usize,
            value: *const u8,
            value_len: usize,
            data: *const u8,
            data_len: usize,
        ) -> i32;

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

    if copied_size == buffer_size {
        data_buffer.truncate(copied_size as usize);
        Ok(data_buffer)
    } else {
        Err(PlaidFunctionError::ReturnBufferTooSmall)
    }
}

/// Read the value at `key` in this rule's namespace.
///
/// Returns an empty vector if the key is not set.
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

/// Read the value at `key` in a shared namespace.
///
/// Returns an empty vector if the key is not set. The rule must have read or read-write access
/// to `namespace` in Plaid's configuration.
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

/// List all keys in this rule's namespace.
///
/// If `prefix` is given, only keys starting with that prefix are returned.
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

/// List all keys in a shared namespace.
///
/// If `prefix` is given, only keys starting with that prefix are returned. The rule must have
/// read access to the shared namespace in Plaid's configuration.
pub fn list_keys_shared(
    namespace: &str,
    prefix: Option<impl Display>,
) -> Result<Vec<String>, PlaidFunctionError> {
    extern "C" {
        fn storage_list_keys_shared(
            namespace: *const u8,
            namespace_len: usize,
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

    let namespace_bytes = namespace.as_bytes().to_vec();
    let prefix_bytes = prefix.as_bytes().to_vec();

    let buffer_size = unsafe {
        storage_list_keys_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
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
        storage_list_keys_shared(
            namespace_bytes.as_ptr(),
            namespace_bytes.len(),
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

/// Delete the value at `key` in this rule's namespace.
///
/// Returns the value that was stored at `key`. If the key was not set, returns an empty vector.
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

/// Delete the value at `key` in a shared namespace.
///
/// Returns the value that was stored at `key`. If the key was not set, returns an empty vector.
/// The rule must have read-write access to `namespace` in Plaid's configuration.
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
