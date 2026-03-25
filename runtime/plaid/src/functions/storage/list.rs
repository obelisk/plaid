use std::sync::Arc;

use wasmer::{AsStoreRef, FunctionEnvMut, MemoryView, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors, storage::Storage};

use super::{get_memory, safely_get_string, safely_write_data_back};

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

    let Some(storage) = &env_data.storage else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_list_keys: {e:?}",
                env_data.module.name,
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    safely_get_guest_string!(prefix, memory_view, prefix_buf, prefix_buf_len, env_data);

    list_keys_common(
        env_data,
        storage,
        env_data.module.name.clone(),
        prefix,
        memory_view,
        data_buffer,
        data_buffer_len,
    )
}

/// Fetch all the keys from a shared namespace in the storage system and filter for a prefix
/// before returning the data.
pub fn list_keys_shared(
    env: FunctionEnvMut<Env>,
    namespace_buf: WasmPtr<u8>,
    namespace_buf_len: u32,
    prefix_buf: WasmPtr<u8>,
    prefix_buf_len: u32,
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
                "{}: Memory error in storage_list_keys: {e:?}",
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

    // Check if we can access this namespace, otherwise we just stop
    let allowed = match shared_dbs.get(&namespace) {
        None => false,
        Some(db) => {
            db.config.r.contains(&env_data.module.name)
                || db.config.rw.contains(&env_data.module.name)
        }
    };
    if !allowed {
        return FunctionErrors::OperationNotAllowed as i32;
    }

    safely_get_guest_string!(prefix, memory_view, prefix_buf, prefix_buf_len, env_data);

    list_keys_common(
        env_data,
        storage,
        namespace,
        prefix,
        memory_view,
        data_buffer,
        data_buffer_len,
    )
}

/// Code which is common to `list_keys` and `list_keys_shared`
fn list_keys_common(
    env_data: &Env,
    storage: &Arc<Storage>,
    namespace: String,
    prefix: String,
    memory_view: MemoryView,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.list_keys(&namespace, Some(prefix.as_str())).await });

    match result {
        Ok(keys) => {
            let serialized_keys = match serde_json::to_string(&keys) {
                Ok(sk) => sk,
                Err(e) => {
                    error!(
                        "Could not serialize keys for namespaces {}: {e}",
                        env_data.module.name
                    );
                    return FunctionErrors::ErrorCouldNotSerialize as i32;
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
                        "{}: Data write error in storage_list: {e:?}",
                        env_data.module.name,
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
            return FunctionErrors::InternalApiError as i32;
        }
    }
}
