use std::sync::Arc;

use wasmer::{AsyncFunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, storage::Storage};

use super::{safely_get_string_async, safely_write_data_back_async};

/// Fetch all the keys from the storage system and filter for a prefix
/// before returning the data.
pub async fn list_keys(
    env: AsyncFunctionEnvMut<Env>,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match list_keys_impl(&env, prefix_buf, prefix_buf_len, data_buffer, data_buffer_len).await {
        Ok(res) => res,
        Err(e) => {
            error!("storage_list_keys experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn list_keys_impl(
    env: &AsyncFunctionEnvMut<Env>,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the prefix from guest memory. The
    //    guard is dropped at the end of this block.
    let (module_name, storage, prefix) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let Some(storage) = &env_data.storage else {
            return Err(FunctionErrors::ApiNotConfigured);
        };

        let prefix = match safely_get_string_async(env, prefix_buf, prefix_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    env_data.module.name, e
                );
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (env_data.module.name.clone(), storage.clone(), prefix)
    };

    list_keys_common(
        &module_name,
        &storage,
        module_name.clone(),
        prefix,
        env,
        data_buffer,
        data_buffer_len,
    )
    .await
}

/// Fetch all the keys from a shared namespace in the storage system and filter for a prefix
/// before returning the data.
pub async fn list_keys_shared(
    env: AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match list_keys_shared_impl(
        &env,
        namespace_buf,
        namespace_buf_len,
        prefix_buf,
        prefix_buf_len,
        data_buffer,
        data_buffer_len,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("storage_list_keys_shared experienced an issue: {:?}", e);
            e as i32
        }
    }
}

async fn list_keys_shared_impl(
    env: &AsyncFunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 1) Guard: snapshot what we need, read the namespace and prefix from
    //    guest memory, and check namespace access permissions. The guard is
    //    dropped at the end of this block.
    let (module_name, storage, namespace, prefix) = {
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

        let prefix = match safely_get_string_async(env, prefix_buf, prefix_buf_len).await {
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
            prefix,
        )
    };

    list_keys_common(
        &module_name,
        &storage,
        namespace,
        prefix,
        env,
        data_buffer,
        data_buffer_len,
    )
    .await
}

/// Code which is common to `list_keys` and `list_keys_shared`
async fn list_keys_common(
    module_name: &str,
    storage: &Arc<Storage>,
    namespace: String,
    prefix: String,
    env: &AsyncFunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> Result<i32, FunctionErrors> {
    // 2) NO guard held: the await below suspends the guest's stack and the
    //    executor thread is free to run other guests.
    let result = storage.list_keys(&namespace, Some(prefix.as_str())).await;

    match result {
        Ok(keys) => {
            let serialized_keys = match serde_json::to_string(&keys) {
                Ok(sk) => sk,
                Err(e) => {
                    error!("Could not serialize keys for namespaces {module_name}: {e}");
                    return Err(FunctionErrors::ErrorCouldNotSerialize);
                }
            };

            match safely_write_data_back_async(
                env,
                &serialized_keys.as_bytes(),
                data_buffer,
                data_buffer_len,
            )
            .await
            {
                Ok(x) => Ok(x),
                Err(e) => {
                    error!("{}: Data write error in storage_list: {:?}", module_name, e);
                    Err(e)
                }
            }
        }
        Err(e) => {
            error!("Could not list keys for namespace {module_name}: {e}");
            Err(FunctionErrors::InternalApiError)
        }
    }
}
