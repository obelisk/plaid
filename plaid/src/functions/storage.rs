use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors};

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
    let result = env_data.api.clone().runtime.block_on(async move {
        storage
            .insert(env_data.name.clone(), storage_key, value)
            .await
    });

    match result {
        Ok(Some(data)) => {
            // If the data is too large to fit in the buffer that was passed to us. Unfortunately this is a somewhat
            // unrecoverable state because we've overwritten the value already. We could fail insertion if the data
            // buffer passed is too small in future? That would mean doing a get call first, which the client can do
            // too.
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
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
        Ok(None) => 0,
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

/// Store data in the storage system if one is configured
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
            error!("{}: Memory error in storage_list_keys: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let prefix = match safely_get_string(&memory_view, prefix_buf, prefix_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Prefix error in storage_list_keys: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.list_keys(&env_data.name, Some(prefix.as_str())).await });

    match result {
        Ok(keys) => {
            let serialized_keys = match serde_json::to_string(&keys) {
                Ok(sk) => sk,
                Err(e) => {
                    error!("Could not serialize keys for namespaces {}: {e}", &env_data.name);
                    return 0;
                }
            };

            match safely_write_data_back(&memory_view, &serialized_keys.as_bytes(), data_buffer, data_buffer_len) {
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
        },
    }
}