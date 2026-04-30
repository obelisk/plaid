use std::sync::{Arc, RwLock};

use wasmer::{AsStoreRef, FunctionEnvMut, MemoryView, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, loader::LimitValue, storage::Storage};

use super::{
    calculate_max_buffer_size, get_memory, safely_get_memory, safely_get_string,
    safely_write_data_back,
};

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

    let Some(storage) = &env_data.storage else {
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

    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);
    safely_get_guest_memory!(value, memory_view, value_buf, value_buf_len, env_data);

    match insert_common(
        env_data,
        storage,
        env_data.module.name.clone(),
        key,
        value,
        memory_view,
        data_buffer,
        data_buffer_len,
        env_data.module.storage_limit.clone(),
        env_data.module.storage_current.clone(),
    ) {
        Ok(code) => code,
        Err(e) => e as i32,
    }
}

/// Store data in a shared namespace in the storage system, if one is configured
pub fn insert_shared(
    env: FunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let Some(storage) = &env_data.storage else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    // Check if we have shared DBs at all, otherwise we just stop
    let Some(shared_dbs) = &storage.shared_dbs else {
        return FunctionErrors::OperationNotAllowed as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_insert_shared: {e:?}",
                env_data.module.name,
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    safely_get_guest_string!(
        namespace,
        memory_view,
        namespace_buf,
        namespace_buf_len,
        env_data
    );

    // Get the shared DB, if it exists. Otherwise, exit with an error
    let Some(db) = shared_dbs.get(&namespace) else {
        return FunctionErrors::SharedDbError as i32;
    };

    // Check if calling module has permission to write to the DB
    if !db.config.rw.contains(&env_data.module.name) {
        return FunctionErrors::OperationNotAllowed as i32;
    }

    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);
    safely_get_guest_memory!(value, memory_view, value_buf, value_buf_len, env_data);

    match insert_common(
        env_data,
        storage,
        namespace,
        key,
        value,
        memory_view,
        data_buffer,
        data_buffer_len,
        db.config.size_limit.clone(),
        db.used_storage.clone(),
    ) {
        Ok(code) => code,
        Err(e) => e as i32,
    }
}

/// Code which is common to `insert` and `insert_shared`
fn insert_common(
    env_data: &Env,
    storage: &Arc<Storage>,
    namespace: String,
    key: String,
    value: Vec<u8>,
    memory_view: MemoryView,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
    storage_limit: LimitValue,
    storage_counter: Arc<RwLock<u64>>,
) -> Result<i32, FunctionErrors> {
    // The insertion proceeds differently depending on whether the storage is limited or not.
    let insertion_result = match storage_limit {
        LimitValue::Unlimited => {
            // The storage is unlimited, so we don't check / update any counters and just proceed with the operation
            env_data
                .api
                .clone()
                .runtime
                .block_on(async move { storage.insert(namespace, key, value).await })
        }
        LimitValue::Limited(limit) => {
            // The storage is limited, so we need to check / update counters (with locks) because the operation might have to be rejected.

            let existing_data_size = fetch_existing_data_size(env_data, storage, &namespace, &key)?;

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
                env_data,
                *storage_current,
                existing_data_size,
                key.as_bytes().len() as u64,
                value.len() as u64,
                limit,
                &key,
            )?;

            let result = env_data
                .api
                .clone()
                .runtime
                .block_on(async move { storage.insert(namespace, key, value).await });

            // If the insertion went well, update counter for used storage.
            // If the insertion failed for some reason, we don't update the counter and release the lock: no harm done.
            if result.is_ok() {
                *storage_current = would_be_used_storage;
            }
            result
        }
    };

    handle_insertion_result(
        env_data,
        insertion_result,
        &memory_view,
        data_buffer,
        data_buffer_len,
    )
}

/// Writes the previously-stored value (returned by the storage insert) back to guest memory
/// and returns the number of bytes written, or an appropriate error code.
fn handle_insertion_result(
    env_data: &Env,
    insertion_result: Result<Option<Vec<u8>>, impl std::fmt::Display>,
    memory_view: &MemoryView,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    match insertion_result {
        Ok(Some(data)) => {
            // If the data is too large to fit in the buffer that was passed to us. Unfortunately this is a somewhat
            // unrecoverable state because we've overwritten the value already. We could fail insertion if the data
            // buffer passed is too small in future? That would mean doing a get call first, which the client can do
            // too.
            safely_write_data_back(memory_view, &data, data_buffer, data_buffer_len).inspect_err(
                |e| {
                    error!(
                        "{}: Data write error in storage_insert: {e:?}",
                        env_data.module.name,
                    );
                },
            )
        }
        // No previous value for this key; report zero bytes written back.
        Ok(None) => Ok(0),
        // If the storage system errors (for example a network problem if using a networked storage provider)
        // the error is made opaque to the client here and we log what happened
        Err(e) => {
            error!(
                "There was a storage system error during insert by [{}]: {e}",
                env_data.module.name
            );
            Err(FunctionErrors::InternalApiError)
        }
    }
}

/// Fetches the number of bytes currently occupied by an existing key (value length + key length),
/// or 0 if the key does not exist. Returns `Err(i32)` with a ready-to-return error code on failure.
fn fetch_existing_data_size(
    env_data: &Env,
    storage: &Arc<Storage>,
    namespace: &str,
    key: &str,
) -> Result<u64, FunctionErrors> {
    let key_len = key.as_bytes().len() as u64;
    match env_data
        .api
        .clone()
        .runtime
        .block_on(async { storage.get(namespace, key).await })
    {
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
    env_data: &Env,
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
            "{}: Could not insert key/value with key [{key}] as that would bring us above the configured storage limit.",
            env_data.module.name
        );
        let _ = env_data.external_logging_system.log_module_error(
            env_data.module.name.clone(),
            "Could not insert key/value as that would bring us above the configured storage limit."
                .to_string(),
            vec![],
        );
        return Err(FunctionErrors::StorageLimitReached);
    }

    Ok(would_be_used_storage)
}
