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
            error!(
                "{}: Memory error in storage_insert: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Key error in storage_insert: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    // Get the storage data from the client's memory
    let value = match safely_get_memory(&memory_view, value_buf, value_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!(
                "{}: Value error in storage_insert: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let storage_key = key.clone();

    // The insertion proceeds differently depending on whether the module's storage is limited or not.
    let insertion_result = match env_data.module.storage_limit {
        LimitValue::Unlimited => {
            // The module has unlimited storage, so we don't check / update any counters and just proceed with the operation
            env_data.api.clone().runtime.block_on(async move {
                storage
                    .insert(env_data.module.name.clone(), storage_key, value)
                    .await
            })
        }
        LimitValue::Limited(storage_limit) => {
            // The module has limited storage, so we need to check / update counters (with locks) because the operation might have to be rejected.

            // Get a lock on the storage counter for the module that is processing this message.
            // This ensures no race conditions if multiple instances of the same module are running in parallel.
            // The guard will go out of scope at the end of this block, thus releasing the lock. After this block, we won't touch the counter again.
            let mut storage_current = match env_data.module.storage_current.write() {
                Ok(g) => g,
                Err(e) => {
                    error!("Critical error getting a lock on used storage: {:?}", e);
                    return FunctionErrors::InternalApiError as i32;
                }
            };

            // We check if this insert would overwrite some existing data. If so, we need to take that into account when
            // computing the storage that would be occupied at the end of the insert operation.
            // Note: if we have existing data, then we need to count the key's length as well. This is because at the end
            // of a possible insertion, we would have only one key.
            let get_key = key.clone();
            let key_len = key.as_bytes().len();
            let existing_data_size = match env_data
                .api
                .clone()
                .runtime
                .block_on(async move { storage.get(&env_data.module.name, &get_key).await })
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
            let would_be_used_storage =
                *storage_current + key_len as u64 + value.len() as u64 - existing_data_size;
            // no problem with underflowing because the result will never be negative (since *storage_current >= existing_data_size)

            // If we would go above the limited storage, reject the insert
            if would_be_used_storage > storage_limit {
                error!("{}: Could not insert key/value with key {storage_key} as that would bring us above the configured storage limit.", env_data.module.name);
                return FunctionErrors::StorageLimitReached as i32;
            }

            let result = env_data.api.clone().runtime.block_on(async move {
                storage
                    .insert(env_data.module.name.clone(), storage_key, value)
                    .await
            });
            // If the insertion went well, update counter for used storage.
            // If the insertion failed for some reason, we don't update the counter and release the lock: no harm done.
            if result.is_ok() {
                *storage_current = would_be_used_storage;
            }
            result
        }
    };

    // Process the insertion result and return info to the caller
    match insertion_result {
        Ok(data) => {
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
                                env_data.module.name, e
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
                env_data.module.name
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
            error!(
                "{}: Memory error in storage_get: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Key error in storage_get: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.get(&env_data.module.name, &key).await });

    match result {
        Ok(Some(data)) => {
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_get: {:?}",
                        env_data.module.name, e
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
                env_data.module.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let prefix = match safely_get_string(&memory_view, prefix_buf, prefix_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Prefix error in storage_list_keys: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data.api.clone().runtime.block_on(async move {
        storage
            .list_keys(&env_data.module.name, Some(prefix.as_str()))
            .await
    });

    match result {
        Ok(keys) => {
            let serialized_keys = match serde_json::to_string(&keys) {
                Ok(sk) => sk,
                Err(e) => {
                    error!(
                        "Could not serialize keys for namespaces {}: {e}",
                        &env_data.module.name
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
                        env_data.module.name, e
                    );
                    e as i32
                }
            }
        }
        Err(e) => {
            error!(
                "Could not list keys for namespace {}: {e}",
                &env_data.module.name
            );
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
            error!(
                "{}: Memory error in storage_delete: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Key error in storage_delete: {:?}",
                env_data.module.name, e
            );
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let key_len = key.as_bytes().len();

    let deletion_result = match data_buffer_len {
        // This is a call just to get the size of the buffer, so we do storage.get and don't mess with storage counters
        0 => env_data
            .api
            .clone()
            .runtime
            .block_on(async move { storage.get(&env_data.module.name, &key).await }),
        // This is a call to delete the value, so we will do storage.delete, but first we need to check the storage limit
        _ => match env_data.module.storage_limit {
            LimitValue::Unlimited => {
                // The module has unlimited storage, so we don't update any counters and just proceed with the operation
                env_data
                    .api
                    .clone()
                    .runtime
                    .block_on(async move { storage.delete(&env_data.module.name, &key).await })
            }
            LimitValue::Limited(_) => {
                // The module has limited storage, so we need to update counters (with locks)

                // Get a lock on the storage counter for the module that is processing this message.
                // This ensures no race conditions if multiple instances of the same module are running in parallel.
                // The guard will go out of scope at the end of this block, thus releasing the lock.  After this block, we won't touch the counter again.
                let mut storage_current = match env_data.module.storage_current.write() {
                    Ok(g) => g,
                    Err(e) => {
                        error!("Critical error getting a lock on used storage: {:?}", e);
                        return FunctionErrors::InternalApiError as i32;
                    }
                };

                let result = env_data
                    .api
                    .clone()
                    .runtime
                    .block_on(async move { storage.delete(&env_data.module.name, &key).await });
                // If the deletion went well, update counter for used storage.
                // If the deletion failed for some reason, we don't update the counter and release the lock: no harm done.
                if let Ok(Some(ref data)) = result {
                    *storage_current = *storage_current - key_len as u64 - data.len() as u64;
                }
                result
            }
        },
    };

    // Process the deletion result and return info to the caller
    match deletion_result {
        Ok(data) => match data {
            Some(data) => {
                match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                    Ok(x) => x,
                    Err(e) => {
                        error!(
                            "{}: Data write error in storage_delete: {:?}",
                            env_data.module.name, e
                        );
                        e as i32
                    }
                }
            }
            None => 0,
        },
        Err(_) => 0,
    }
}
