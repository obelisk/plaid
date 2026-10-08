use std::sync::{Arc, RwLock};

use plaid_stl::plaid::storage::Item;
use wasmer::{AsyncFunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, loader::LimitValue, storage::Storage};

use super::{
    calculate_max_buffer_size, safely_get_memory_async, safely_get_string_async,
    safely_write_data_back_async,
};

/// Insert multiple key/value pairs into the storage system in a single batch operation.
///
/// The guest passes a single JSON-encoded `Vec<Item>` in `items_buf`.
/// Returns 0 on success.
pub async fn insert_batch(
    env: AsyncFunctionEnvMut<Env>,
    items_buf: WasmPtr<u8>,
    items_buf_len: u32,
) -> i32 {
    match insert_batch_impl(&env, items_buf, items_buf_len).await {
        Ok(res) => res,
        Err(e) => {
            error!("storage_insert_batch experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn insert_batch_impl(
    env: &AsyncFunctionEnvMut<Env>,
    items_buf: WasmPtr<u8>,
    items_buf_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the items JSON from guest memory.
    //    The guard is dropped at the end of this block.
    let (module_name, els, storage, items_json, storage_limit, counter) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let Some(counter) = env_data.module.storage_current.clone() else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let items_json = match safely_get_string_async(env, items_buf, items_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (
            env_data.module.name.clone(),
            env_data.external_logging_system.clone(),
            storage.clone(),
            items_json,
            env_data.module.storage_limit.clone(),
            counter,
        )
    };

    insert_batch_common(
        &module_name,
        &els,
        &storage,
        module_name.clone(),
        items_json,
        storage_limit,
        counter,
    )
    .await
}


/// Insert multiple key/value pairs into a shared namespace in a single batch operation.
///
/// The guest passes a single JSON-encoded `Vec<Item>` in `items_buf`.
/// Returns 0 on success.
pub async fn insert_batch_shared(
    env: AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    items_buf: WasmPtr<u8>,
    items_buf_len: u32,
) -> i32 {
    match insert_batch_shared_impl(&env, namespace_buf, namespace_buf_len, items_buf, items_buf_len)
        .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("storage_insert_batch_shared experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn insert_batch_shared_impl(
    env: &AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    items_buf: WasmPtr<u8>,
    items_buf_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the namespace and items JSON
    //    from guest memory, and check namespace write permissions. The
    //    guard is dropped at the end of this block.
    let (module_name, els, storage, namespace, items_json, storage_limit, counter) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let Some(shared_dbs) = &storage.shared_dbs else {
            return Err(FunctionErrors::OperationNotAllowed);
        };

        let namespace =
            match safely_get_string_async(env, namespace_buf, namespace_buf_len).await {
                Ok(s) => s,
                Err(e) => {
                    error!(
                        "{}: error while getting a string from guest memory: {:?}",
                        env_data.module.name, e
                    );
                    return Err(FunctionErrors::ParametersNotUtf8);
                }
            };

        let Some(db) = shared_dbs.get(&namespace) else {
            return Err(FunctionErrors::SharedDbError);
        };

        if !db.config.rw.contains(&env_data.module.name) {
            return Err(FunctionErrors::OperationNotAllowed);
        }

        let items_json = match safely_get_string_async(env, items_buf, items_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (
            env_data.module.name.clone(),
            env_data.external_logging_system.clone(),
            storage.clone(),
            namespace,
            items_json,
            db.config.size_limit.clone(),
            db.used_storage.clone(),
        )
    };

    insert_batch_common(
        &module_name,
        &els,
        &storage,
        namespace,
        items_json,
        storage_limit,
        counter,
    )
    .await
}

/// Code common to [`insert_batch`] and [`insert_batch_shared`].
///
/// `items_json` is a JSON-encoded `Vec<Item>`.
/// Storage-limit accounting mirrors [`insert_common`]: for each item we subtract the size of any
/// existing data that would be overwritten.
async fn insert_batch_common(
    module_name: &str,
    els: &crate::logging::Logger,
    storage: &Arc<Storage>,
    namespace: String,
    items_json: String,
    storage_limit: LimitValue,
    storage_counter: Arc<RwLock<u64>>,
) -> Result<i32, FunctionErrors> {
    let items: Vec<Item> = match serde_json::from_str(&items_json) {
        Ok(v) => v,
        Err(e) => {
            error!(
                "{module_name}: Failed to deserialize items for storage_insert_batch: {e}",
            );
            return Err(FunctionErrors::ErrorCouldNotSerialize);
        }
    };

    info!(
        "[{module_name}]: batch inserting {} items to namespace {namespace}",
        items.len()
    );

    // For a limited namespace, check that the entire batch would fit before writing anything.
    match storage_limit {
        LimitValue::Unlimited => {
            // The storage is unlimited, so we don't check / update any counters and just proceed with the operation
            let result = storage.insert_batch(namespace, items).await;

            match result {
                Ok(()) => Ok(0),
                Err(e) => {
                    error!("{module_name}: Storage error during insert_batch: {e}",);
                    Err(FunctionErrors::InternalApiError)
                }
            }
        }
        LimitValue::Limited(limit) => {
            let mut storage_current = match storage_counter.write() {
                Ok(g) => g,
                Err(e) => {
                    error!("Critical error getting a lock on used storage: {e:?}");
                    return Err(FunctionErrors::InternalApiError);
                }
            };

            // Compute the net byte delta for the whole batch, accounting for any data that would
            // be overwritten by keys that already exist.
            let mut net_delta: i64 = 0;
            for item in &items {
                let key_len = item.key.as_bytes().len() as u64;
                let existing =
                    fetch_existing_data_size(storage, &namespace, &item.key).await?;

                // New contribution: key + value. Subtract what was already counted.
                net_delta += (key_len + item.value.len() as u64) as i64 - existing as i64;
            }

            let would_be_used = (*storage_current as i64 + net_delta) as u64;
            if would_be_used > limit {
                error!(
                    "{module_name}: Batch insert rejected: would exceed the configured storage limit.",
                );
                let _ = els.log_module_error(
                    module_name.to_string(),
                    "Batch insert rejected: would exceed the configured storage limit.".to_string(),
                    vec![],
                );
                return Err(FunctionErrors::StorageLimitReached);
            }

            let result = storage.insert_batch(namespace, items).await;

            match result {
                Ok(()) => {
                    *storage_current = would_be_used;
                    Ok(0)
                }
                Err(e) => {
                    error!("{module_name}: Storage error during insert_batch: {e}",);
                    Err(FunctionErrors::InternalApiError)
                }
            }
        }
    }
}

/// Store data in the storage system if one is configured
pub async fn insert(
    env: AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match insert_impl(
        &env,
        key_buf,
        key_buf_len,
        value_buf,
        value_buf_len,
        data_buffer,
        data_buffer_len,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("storage_insert experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn insert_impl(
    env: &AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the key and value from guest
    //    memory. The guard is dropped at the end of this block.
    let (module_name, storage, key, value, storage_limit, counter) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let Some(counter) = env_data.module.storage_current.clone() else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let key = match safely_get_string_async(env, key_buf, key_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        let max_buffer_size = calculate_max_buffer_size(env_data.module.page_limit);
        let value = match safely_get_memory_async(env, value_buf, value_buf_len, max_buffer_size).await
        {
            Ok(d) => d,
            Err(e) => {
                error!(
                    "{}: error while getting bytes from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (
            env_data.module.name.clone(),
            storage.clone(),
            key,
            value,
            env_data.module.storage_limit.clone(),
            counter,
        )
    };

    insert_common(
        &module_name,
        &storage,
        module_name.clone(),
        key,
        value,
        env,
        data_buffer,
        data_buffer_len,
        storage_limit,
        counter,
    )
    .await
}

/// Store data in a shared namespace in the storage system, if one is configured
pub async fn insert_shared(
    env: AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match insert_shared_impl(
        &env,
        namespace_buf,
        namespace_buf_len,
        key_buf,
        key_buf_len,
        value_buf,
        value_buf_len,
        data_buffer,
        data_buffer_len,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("storage_insert_shared experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn insert_shared_impl(
    env: &AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the namespace, key, and value
    //    from guest memory, and check namespace write permissions. The
    //    guard is dropped at the end of this block.
    let (module_name, storage, namespace, key, value, storage_limit, counter) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        // Check if we have shared DBs at all, otherwise we just stop
        let Some(shared_dbs) = &storage.shared_dbs else {
            return Err(FunctionErrors::OperationNotAllowed);
        };

        let namespace =
            match safely_get_string_async(env, namespace_buf, namespace_buf_len).await {
                Ok(s) => s,
                Err(e) => {
                    error!(
                        "{}: error while getting a string from guest memory: {:?}",
                        env_data.module.name, e
                    );
                    return Err(FunctionErrors::ParametersNotUtf8);
                }
            };

        // Get the shared DB, if it exists. Otherwise, exit with an error
        let Some(db) = shared_dbs.get(&namespace) else {
            return Err(FunctionErrors::SharedDbError);
        };

        // Check if calling module has permission to write to the DB
        if !db.config.rw.contains(&env_data.module.name) {
            return Err(FunctionErrors::OperationNotAllowed);
        }

        let key = match safely_get_string_async(env, key_buf, key_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        let max_buffer_size = calculate_max_buffer_size(env_data.module.page_limit);
        let value = match safely_get_memory_async(env, value_buf, value_buf_len, max_buffer_size).await
        {
            Ok(d) => d,
            Err(e) => {
                error!(
                    "{}: error while getting bytes from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (
            env_data.module.name.clone(),
            storage.clone(),
            namespace,
            key,
            value,
            db.config.size_limit.clone(),
            db.used_storage.clone(),
        )
    };

    insert_common(
        &module_name,
        &storage,
        namespace,
        key,
        value,
        env,
        data_buffer,
        data_buffer_len,
        storage_limit,
        counter,
    )
    .await
}

/// Code which is common to `insert` and `insert_shared`
async fn insert_common(
    module_name: &str,
    storage: &Arc<Storage>,
    namespace: String,
    key: String,
    value: Vec<u8>,
    env: &AsyncFunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
    storage_limit: LimitValue,
    storage_counter: Arc<RwLock<u64>>,
) -> Result<i32, FunctionErrors> {
    // The insertion proceeds differently depending on whether the storage is limited or not.
    // The awaits below suspend the guest's stack and free the executor thread.
    let insertion_result = match storage_limit {
        LimitValue::Unlimited => {
            // The storage is unlimited, so we don't check / update any counters and just proceed with the operation
            storage.insert(namespace, key, value).await
        }
        LimitValue::Limited(limit) => {
            // The storage is limited, so we need to check / update counters (with locks) because the operation might have to be rejected.

            let existing_data_size =
                fetch_existing_data_size(storage, &namespace, &key).await?;

            // Get a lock on the storage counter.
            // This ensures no race conditions if multiple instances of the same module are running in parallel.
            // The guard is held until the end of this block so that the counter update and the
            // actual insertion are atomic with respect to other concurrent module instances.
            let mut storage_current = match storage_counter.write() {
                Ok(g) => g,
                Err(e) => {
                    error!("Critical error getting a lock on used storage: {e:?}");
                    return Err(FunctionErrors::InternalApiError);
                }
            };

            let would_be_used_storage = check_storage_limit(
                module_name,
                *storage_current,
                existing_data_size,
                key.as_bytes().len() as u64,
                value.len() as u64,
                limit,
                &key,
            )?;

            let result = storage.insert(namespace, key, value).await;

            // If the insertion went well, update counter for used storage.
            // If the insertion failed for some reason, we don't update the counter and release the lock: no harm done.
            if result.is_ok() {
                *storage_current = would_be_used_storage;
            }
            result
        }
    };

    handle_insertion_result(
        module_name,
        insertion_result,
        env,
        data_buffer,
        data_buffer_len,
    )
    .await
}

/// Writes the previously-stored value (returned by the storage insert) back to guest memory
/// and returns the number of bytes written, or an appropriate error code.
async fn handle_insertion_result(
    module_name: &str,
    insertion_result: Result<Option<Vec<u8>>, impl std::fmt::Display>,
    env: &AsyncFunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    match insertion_result {
        Ok(Some(data)) => {
            // If the data is too large to fit in the buffer that was passed to us. Unfortunately this is a somewhat
            // unrecoverable state because we've overwritten the value already. We could fail insertion if the data
            // buffer passed is too small in future? That would mean doing a get call first, which the client can do
            // too.
            safely_write_data_back_async(env, &data, data_buffer, data_buffer_len)
                .await
                .inspect_err(|e| {
                    error!("{module_name}: Data write error in storage_insert: {e:?}",);
                })
        }
        // No previous value for this key; report zero bytes written back.
        Ok(None) => Ok(0),
        // If the storage system errors (for example a network problem if using a networked storage provider)
        // the error is made opaque to the client here and we log what happened
        Err(e) => {
            error!("There was a storage system error during insert by [{module_name}]: {e}");
            Err(FunctionErrors::InternalApiError)
        }
    }
}

/// Fetches the number of bytes currently occupied by an existing key (value length + key length),
/// or 0 if the key does not exist. Returns `Err(i32)` with a ready-to-return error code on failure.
async fn fetch_existing_data_size(
    storage: &Arc<Storage>,
    namespace: &str,
    key: &str,
) -> Result<u64, FunctionErrors> {
    let key_len = key.as_bytes().len() as u64;
    match storage.get(namespace, key).await {
        Ok(None) => Ok(0u64),
        // If we have existing data, count the key length too since at the end of a possible
        // insertion there would still be only one key occupying that space.
        Ok(Some(d)) => Ok(d.len() as u64 + key_len),
        Err(_) => Err(FunctionErrors::InternalApiError),
    }
}

/// Checks whether inserting `new_value_len` bytes under `key` would exceed `storage_limit`.
/// Returns the would-be storage usage on success, or `Err(i32)` with a ready-to-return error
/// code if the limit would be exceeded.
fn check_storage_limit(
    module_name: &str,
    current_storage: u64,
    existing_data_size: u64,
    key_len: u64,
    new_value_len: u64,
    storage_limit: u64,
    key: &str,
) -> Result<u64, FunctionErrors> {
    // Note: we subtract existing_data_size because the old value would be overwritten.
    // No underflow risk: current_storage >= existing_data_size always holds.
    let would_be_used_storage = current_storage + key_len + new_value_len - existing_data_size;

    if would_be_used_storage > storage_limit {
        error!(
            "{module_name}: Could not insert key/value with key [{key}] as that would bring us above the configured storage limit."
        );
        return Err(FunctionErrors::StorageLimitReached);
    }

    Ok(would_be_used_storage)
}
