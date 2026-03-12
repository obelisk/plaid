use std::sync::{Arc, RwLock};

use wasmer::{AsStoreRef, FunctionEnvMut, MemoryView, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, loader::LimitValue, storage::Storage};

use super::{get_memory, safely_get_string, safely_write_data_back};

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

    let Some(storage) = &env_data.storage else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_delete: {e:?}",
                env_data.module.name,
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);

    delete_common(
        env_data,
        storage,
        env_data.module.name.clone(),
        key,
        memory_view,
        data_buffer,
        data_buffer_len,
        env_data.module.storage_limit.clone(),
        env_data.module.storage_current.clone(),
    )
}

/// Delete data from a shared namespace in the storage system, if one is configured
pub fn delete_shared(
    env: FunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
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
                "{}: Memory error in storage_delete: {e:?}",
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
    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);

    // Check if we can access this namespace, otherwise we just stop
    // Get the shared DB, if it exists. Otherwise, exit with an error
    let Some(db) = shared_dbs.get(&namespace) else {
        return FunctionErrors::SharedDbError as i32;
    };

    // Check if calling module has permission to write to the DB
    if !db.config.rw.contains(&env_data.module.name) {
        return FunctionErrors::OperationNotAllowed as i32;
    }

    delete_common(
        env_data,
        storage,
        namespace,
        key,
        memory_view,
        data_buffer,
        data_buffer_len,
        db.config.size_limit.clone(),
        db.used_storage.clone(),
    )
}

/// Code which is common to `delete` and `delete_shared`
fn delete_common(
    env_data: &Env,
    storage: &Arc<Storage>,
    namespace: String,
    key: String,
    memory_view: MemoryView,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
    storage_limit: LimitValue,
    storage_counter: Arc<RwLock<u64>>,
) -> i32 {
    let deletion_result = match data_buffer_len {
        // This is a call just to get the size of the buffer, so we do storage.get and don't mess with storage counters
        0 => env_data
            .api
            .clone()
            .runtime
            .block_on(async move { storage.get(&namespace, &key).await }),
        // This is a call to delete the value, so we will do storage.delete, but first we need to check the storage limit
        _ => match storage_limit {
            LimitValue::Unlimited => {
                // The storage is unlimited, so we don't update any counters and just proceed with the operation
                env_data
                    .api
                    .clone()
                    .runtime
                    .block_on(async move { storage.delete(&namespace, &key).await })
            }
            LimitValue::Limited(_) => {
                // for the "async move"
                let storage_key = key.clone();

                // The storage is limited, so we need to update counters (with locks)

                // Get a lock on the storage counter.
                // This ensures no race conditions if multiple instances of the same module are running in parallel.
                // The guard will go out of scope at the end of this block, thus releasing the lock.  After this block, we won't touch the counter again.
                let mut storage_current = match storage_counter.write() {
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
                    .block_on(async move { storage.delete(&namespace, &storage_key).await });
                // If the deletion went well, update counter for used storage.
                // If the deletion failed for some reason, we don't update the counter and release the lock: no harm done.
                if let Ok(Some(ref data)) = result {
                    let key_len = key.as_bytes().len() as u64;
                    *storage_current = *storage_current - key_len - data.len() as u64;
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
        Err(_) => FunctionErrors::InternalApiError as i32,
    }
}
