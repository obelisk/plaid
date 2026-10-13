use std::sync::{Arc, RwLock};

use wasmer::{AsyncFunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, loader::LimitValue, storage::Storage};

use super::{safely_get_string_async, safely_write_data_back_async};

/// Delete data from the storage system if one is configured
pub async fn delete(
    env: AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match delete_impl(&env, key_buf, key_buf_len, data_buffer, data_buffer_len).await {
        Ok(res) => res,
        Err(e) => {
            error!("storage_delete experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn delete_impl(
    env: &AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the key from guest memory. The
    //    guard is dropped at the end of this block.
    let (module_name, storage, key, storage_limit, counter) = {
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

        (
            env_data.module.name.clone(),
            storage.clone(),
            key,
            env_data.module.storage_limit.clone(),
            counter,
        )
    };

    delete_common(
        &module_name,
        &storage,
        module_name.clone(),
        key,
        env,
        data_buffer,
        data_buffer_len,
        storage_limit,
        counter,
    )
    .await
}

/// Delete data from a shared namespace in the storage system, if one is configured
pub async fn delete_shared(
    env: AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match delete_shared_impl(
        &env,
        namespace_buf,
        namespace_buf_len,
        key_buf,
        key_buf_len,
        data_buffer,
        data_buffer_len,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("storage_delete_shared experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn delete_shared_impl(
    env: &AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the namespace and key from guest
    //    memory, and check namespace access permissions. The guard is
    //    dropped at the end of this block.
    let (module_name, storage, namespace, key, storage_limit, counter) = {
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

        // Check if we can access this namespace, otherwise we just stop
        // Get the shared DB, if it exists. Otherwise, exit with an error
        let Some(db) = shared_dbs.get(&namespace) else {
            return Err(FunctionErrors::SharedDbError);
        };

        // Check if calling module has permission to write to the DB
        if !db.config.rw.contains(&env_data.module.name) {
            return Err(FunctionErrors::OperationNotAllowed);
        }

        (
            env_data.module.name.clone(),
            storage.clone(),
            namespace,
            key,
            db.config.size_limit.clone(),
            db.used_storage.clone(),
        )
    };

    delete_common(
        &module_name,
        &storage,
        namespace,
        key,
        env,
        data_buffer,
        data_buffer_len,
        storage_limit,
        counter,
    )
    .await
}

/// Code which is common to `delete` and `delete_shared`
async fn delete_common(
    module_name: &str,
    storage: &Arc<Storage>,
    namespace: String,
    key: String,
    env: &AsyncFunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
    storage_limit: LimitValue,
    storage_counter: Arc<RwLock<u64>>,
) -> Result<i32, FunctionErrors> {
    // 2) NO guard held: the awaits below suspend the guest's stack and the
    //    executor thread is free to run other guests.
    let deletion_result = match data_buffer_len {
        // This is a call just to get the size of the buffer, so we do storage.get and don't mess with storage counters
        0 => storage.get(&namespace, &key).await,
        // This is a call to delete the value, so we will do storage.delete, but first we need to check the storage limit
        _ => match storage_limit {
            LimitValue::Unlimited => {
                // The storage is unlimited, so we don't update any counters and just proceed with the operation
                storage.delete(&namespace, &key).await
            }
            LimitValue::Limited(_) => {
                // The storage is limited, so we need to update counters (with locks)

                // Get a lock on the storage counter.
                // This ensures no race conditions if multiple instances of the same module are running in parallel.
                // The guard will go out of scope at the end of this block, thus releasing the lock.  After this block, we won't touch the counter again.
                let mut storage_current = match storage_counter.write() {
                    Ok(g) => g,
                    Err(e) => {
                        error!("Critical error getting a lock on used storage: {:?}", e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                let result = storage.delete(&namespace, &key).await;
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
                match safely_write_data_back_async(env, &data, data_buffer, data_buffer_len).await
                {
                    Ok(x) => Ok(x),
                    Err(e) => {
                        error!("{}: Data write error in storage_delete: {:?}", module_name, e);
                        Err(e)
                    }
                }
            }
            None => Ok(0),
        },
        Err(_) => Err(FunctionErrors::InternalApiError),
    }
}
