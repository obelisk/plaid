use wasmer::{AsStoreRef, FunctionEnvMut, MemoryView, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors};

use super::{get_memory, safely_get_string, safely_write_data_back};

/// Get data from the storage system
pub fn get(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_get: {e:?}",
                env_data.module.name,
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);

    get_common(
        env_data,
        &env_data.module.name,
        &key,
        memory_view,
        data_buffer,
        data_buffer_len,
    )
}

/// Get data from a shared namespace in the storage system
pub fn get_shared(
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

    let storage = &env_data.storage;

    // Check if we have shared DBs at all, otherwise we just stop
    let Some(shared_dbs) = &storage.shared_dbs else {
        return FunctionErrors::OperationNotAllowed as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in storage_get_shared: {e:?}",
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

    safely_get_guest_string!(key, memory_view, key_buf, key_buf_len, env_data);

    get_common(
        env_data,
        &namespace,
        &key,
        memory_view,
        data_buffer,
        data_buffer_len,
    )
}

/// Code which is common to `get` and `get_shared`
fn get_common(
    env_data: &Env,
    namespace: &str,
    key: &str,
    memory_view: MemoryView,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { env_data.storage.get(namespace, key).await });

    match result {
        Ok(Some(data)) => {
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_get: {e:?}",
                        env_data.module.name,
                    );
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(_) => FunctionErrors::InternalApiError as i32,
    }
}
