use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, loader::LimitValue};

use super::{get_memory, safely_get_memory, safely_get_string, safely_write_data_back};

/// Store data in the storage system if one is configured
pub fn insert(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    // Get the storage data from the client's memory
    let value = match safely_get_memory(&memory_view, value_buf, value_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!("{}: Value error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let storage_key = key.clone();
    let get_key = key.clone();
    let key_len = storage_key.as_bytes().len();

    // We check if this insert would overwrite some existing data. If so, we need to take that into account when
    // computing the storage that would be occupied at the end of the insert operation.
    // Note: if we have existing data, then we need to count the key's length as well. This is because at the end
    // of a possible insertion, we would have only one key.
    let existing_data_size = match env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.get(&env_data.name, &get_key).await })
    {
        Ok(data) => match data {
            None => 0u64,
            Some(d) => d.len() as u64 + key_len as u64,
        },
        Err(_) => {
            return FunctionErrors::InternalApiError as i32;
        }
    };

    // Calculate the amount of storage that would be used after successfully inserting.
    // Note: we _substract_ the size of existing data. If we were to insert the new data, the old data would be overwritten.
    let used_storage = match env_data.storage_current.read() {
        Ok(data) => *data,
        Err(_) => panic!(),
    };
    let would_be_used_storage =
        used_storage + key_len as u64 + value.len() as u64 - existing_data_size; // no problem with underflowing because the result will never be negative (since used_storage >= existing_data_size)

    // If we have limited storage, this insert might fail
    if let LimitValue::Limited(storage_limit) = env_data.storage_limit {
        if would_be_used_storage > storage_limit {
            error!("{}: Could not insert key/value with key {storage_key} as that would bring us above the configured storage limit. Used: {used_storage}, would be used: {would_be_used_storage}, limit: {storage_limit}", env_data.name);
            return FunctionErrors::StorageLimitReached as i32;
        }
    }

    let result = env_data.api.clone().runtime.block_on(async move {
        storage
            .insert(env_data.name.clone(), storage_key, value)
            .await
    });

    match result {
        Ok(data) => {
            // The insertion went fine: update counter for used storage
            match env_data.storage_current.write() {
                Ok(mut storage) => {
                    *storage = would_be_used_storage;
                }
                Err(e) => {
                    error!(
                        "Critical error getting a write lock on used storage: {:?}",
                        e
                    );
                    return FunctionErrors::InternalApiError as i32;
                }
            }
            match data {
                Some(data) => {
                    // If the data is too large to fit in the buffer that was passed to us. Unfortunately this is a somewhat
                    // unrecoverable state because we've overwritten the value already. We could fail insertion if the data
                    // buffer passed is too small in future? That would mean doing a get call first, which the client can do
                    // too.
                    match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len)
                    {
                        Ok(x) => x,
                        Err(e) => {
                            error!(
                                "{}: Data write error in storage_insert: {:?}",
                                env_data.name, e
                            );
                            e as i32
                        }
                    }
                }
                // This occurs when there is no such key so the number of bytes that have been copied back are 0
                None => 0,
            }
        }
        // If the storage system errors (for example a network problem if using a networked storage provider)
        // the error is made opaque to the client here and we log what happened
        Err(e) => {
            error!(
                "There was a storage system error when key [{key}] was accessed by [{}]: {e}",
                env_data.name
            );
            return FunctionErrors::InternalApiError as i32;
        }
    }
}

/// Get data from the storage system if one is configured
pub fn get(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in storage_get: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in storage_get: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.get(&env_data.name, &key).await });

    match result {
        Ok(Some(data)) => {
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_get: {:?}",
                        env_data.name, e
                    );
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(_) => 0,
    }
}

/// Fetch all the keys from the storage system and filter for a prefix
/// before returning the data.
pub fn list_keys(
    env: FunctionEnvMut<Env>,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_list_keys: {:?}",
                env_data.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let prefix = match safely_get_string(&memory_view, prefix_buf, prefix_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Prefix error in storage_list_keys: {:?}",
                env_data.name, e
            );
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data.api.clone().runtime.block_on(async move {
        storage
            .list_keys(&env_data.name, Some(prefix.as_str()))
            .await
    });

    match result {
        Ok(keys) => {
            let serialized_keys = match serde_json::to_string(&keys) {
                Ok(sk) => sk,
                Err(e) => {
                    error!(
                        "Could not serialize keys for namespaces {}: {e}",
                        &env_data.name
                    );
                    return 0;
                }
            };

            match safely_write_data_back(
                &memory_view,
                &serialized_keys.as_bytes(),
                data_buffer,
                data_buffer_len,
            ) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_get: {:?}",
                        env_data.name, e
                    );
                    e as i32
                }
            }
        }
        Err(e) => {
            error!("Could not list keys for namespace {}: {e}", &env_data.name);
            return 0;
        }
    }
}

/// Delete data from the storage system if one is configured
pub fn delete(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in storage_delete: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in storage_delete: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let key_len = key.as_bytes().len();

    let result = match data_buffer_len {
        // This is a call just to get the size of the buffer, so we do storage.get
        0 => env_data
            .api
            .clone()
            .runtime
            .block_on(async move { storage.get(&env_data.name, &key).await }),
        // This is a call to delete the value, so we do storage.delete
        _ => env_data
            .api
            .clone()
            .runtime
            .block_on(async move { storage.delete(&env_data.name, &key).await }),
    };

    match result {
        Ok(data) => {
            match data {
                Some(data) => {
                    // Check if we were _actually_ deleting something
                    match data_buffer_len {
                        0 => {}
                        _ => {
                            // We were deleting something, so we decrease the counter for used storage
                            match env_data.storage_current.write() {
                                Ok(mut storage) => {
                                    // no underflow: the result can never become negative
                                    *storage = *storage - key_len as u64 - data.len() as u64;
                                }
                                Err(e) => {
                                    error!(
                                        "Critical error getting a write lock on used storage: {:?}",
                                        e
                                    );
                                    return FunctionErrors::InternalApiError as i32;
                                }
                            }
                        }
                    }
                    match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len)
                    {
                        Ok(x) => x,
                        Err(e) => {
                            error!(
                                "{}: Data write error in storage_delete: {:?}",
                                env_data.name, e
                            );
                            e as i32
                        }
                    }
                }
                None => 0,
            }
        }
        Err(_) => 0,
    }
}
