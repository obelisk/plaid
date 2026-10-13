use std::sync::Arc;

use wasmer::{AsyncFunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, storage::Storage};

use super::{safely_get_string_async, safely_write_data_back_async};

/// Get data from the storage system if one is configured
pub async fn get(
    env: AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match get_impl(&env, key_buf, key_buf_len, data_buffer, data_buffer_len).await {
        Ok(res) => res,
        Err(e) => {
            error!("storage_get experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn get_impl(
    env: &AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the key from guest memory. The
    //    guard is dropped at the end of this block.
    let (module_name, storage, key) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
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

        (env_data.module.name.clone(), storage.clone(), key)
    };

    get_common(
        &module_name,
        &storage,
        &module_name,
        &key,
        env,
        data_buffer,
        data_buffer_len,
    )
    .await
}

/// Get data from a shared namespace in the storage system, if one is configured
pub async fn get_shared(
    env: AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match get_shared_impl(
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
            error!("storage_get_shared experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn get_shared_impl(
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
    let (module_name, storage, namespace, key) = {
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

        // Check if we can access this namespace, otherwise we just stop
        let allowed = match shared_dbs.get(&namespace) {
            None => false,
            Some(db) => {
                db.config.r.contains(&env_data.module.name)
                    || db.config.rw.contains(&env_data.module.name)
            }
        };
        if !allowed {
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

        (
            env_data.module.name.clone(),
            storage.clone(),
            namespace,
            key,
        )
    };

    get_common(
        &module_name,
        &storage,
        &namespace,
        &key,
        env,
        data_buffer,
        data_buffer_len,
    )
    .await
}

/// Code which is common to `get` and `get_shared`
async fn get_common(
    module_name: &str,
    storage: &Arc<Storage>,
    namespace: &str,
    key: &str,
    env: &AsyncFunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 2) NO guard held: the await below suspends the guest's stack and the
    //    executor thread is free to run other guests.
    let result = storage.get(namespace, key).await;

    match result {
        Ok(Some(data)) => {
            match safely_write_data_back_async(env, &data, data_buffer, data_buffer_len).await {
                Ok(x) => Ok(x),
                Err(e) => {
                    error!("{}: Data write error in storage_get: {:?}", module_name, e);
                    Err(e)
                }
            }
        }
        Ok(None) => Ok(0),
        Err(_) => Err(FunctionErrors::InternalApiError),
    }
}
