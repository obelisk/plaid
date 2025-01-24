use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors};

use super::{get_memory, safely_get_string, safely_write_data_back};


/// Store data in the cache system if one is configured
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

    let cache = if let Some(c) = &env_data.module.cache {
        c
    } else {
        return FunctionErrors::CacheDisabled as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in cache_insert: {:?}", env_data.module.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in cache_insert: {:?}", env_data.module.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    // Get the storage data from the client's memory
    let value = match safely_get_string(&memory_view, value_buf, value_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!("{}: Value error in cache_insert: {:?}", env_data.module.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    match cache.write().map(|mut cache| cache.put(key, value)) {
        Ok(Some(previous_value)) => {
            match safely_write_data_back(
                &memory_view,
                &previous_value.as_bytes(),
                data_buffer,
                data_buffer_len,
            ) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in cache_insert: {:?}",
                        env_data.module.name, e
                    );
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(e) => {
            if let Err(e) = env_data.external_logging_system.log_internal_message(
                crate::logging::Severity::Error,
                format!("Cache system error in [{}]: {:?}", env_data.module.name, e),
            ) {
                error!("Logging system is not working!!: {:?}", e);
            }
            FunctionErrors::CacheDisabled as i32
        }
    }
}

/// Get data from the cache system if one is configured
pub fn get(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let cache = if let Some(c) = &env_data.module.cache {
        c
    } else {
        return FunctionErrors::CacheDisabled as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in cache_get: {:?}", env_data.module.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in cache_get: {:?}", env_data.module.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    match cache.write().map(|mut cache| cache.get(&key).cloned()) {
        Ok(Some(value)) => {
            match safely_write_data_back(
                &memory_view,
                &value.as_bytes(),
                data_buffer,
                data_buffer_len,
            ) {
                Ok(x) => x,
                Err(e) => {
                    error!("{}: Data write error in cache_get: {:?}", env_data.module.name, e);
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(e) => {
            if let Err(e) = env_data.external_logging_system.log_internal_message(
                crate::logging::Severity::Error,
                format!("Cache system error in [{}]: {:?}", env_data.module.name, e),
            ) {
                error!("Logging system is not working!!: {:?}", e);
            }
            FunctionErrors::CacheDisabled as i32
        }
    }
}
