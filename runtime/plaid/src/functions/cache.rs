use std::time::Duration;

use tokio::time::timeout;
use wasmer::{AsyncFunctionEnvMut, WasmPtr};

use crate::{
    executor::Env,
    functions::FunctionErrors,
    logging::{Logger, Severity},
};

use super::{safely_get_string_async, safely_write_data_back_async};

/// Store data in the cache system if one is configured
pub async fn insert(
    env: AsyncFunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    value_buf: WasmPtr<u8>,
    value_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    match insert_impl(&env, key_buf, key_buf_len, value_buf, value_buf_len, data_buffer, data_buffer_len).await {
        Ok(res) => res,
        Err(e) => {
            error!("cache_insert experienced an issue: {:?}", e);
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
    // 1) Guard: snapshot what we need (the Logger is a cheap channel clone),
    //    read the key and value from guest memory. The guard is dropped at
    //    the end of this block.
    let (module_name, els, cache, key, value) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let cache = if let Some(c) = &env_data.cache {
            c
        } else {
            return Err(FunctionErrors::CacheDisabled);
        };

        let key = match safely_get_string_async(env, key_buf, key_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!("{}: Key error in cache_insert: {:?}", env_data.module.name, e);
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        // Get the storage data from the client's memory
        let value = match safely_get_string_async(env, value_buf, value_buf_len).await {
            Ok(d) => d,
            Err(e) => {
                error!("{}: Value error in cache_insert: {:?}", env_data.module.name, e);
                return Err(FunctionErrors::CouldNotGetAdequateMemory);
            }
        };

        (
            env_data.module.name.clone(),
            env_data.external_logging_system.clone(),
            cache.clone(),
            key,
            value,
        )
    };

    let namespace = module_name.clone();

    // 2) NO guard held: the await below suspends the guest's stack and the
    //    executor thread is free to run other guests. The timeout now
    //    actually cancels the in-flight work instead of just unblocking
    //    the thread.
    let result = match timeout(
        Duration::from_secs(5),
        cache.put(&namespace, &key, &value),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => return Err(FunctionErrors::TimeoutElapsed),
    };

    // 3) Map the result exactly like the sync version did.
    match result {
        Ok(Some(previous_value)) => {
            match safely_write_data_back_async(
                env,
                &previous_value.as_bytes(),
                data_buffer,
                data_buffer_len,
            )
            .await
            {
                Ok(x) => Ok(x),
                Err(e) => {
                    error!("{}: Data write error in cache_insert: {:?}", module_name, e);
                    Err(e)
                }
            }
        }
        Ok(None) => Ok(0),
        Err(e) => {
            log_cache_error(&els, &module_name, e);
            Err(FunctionErrors::CacheDisabled)
        }
    }
}

/// Get data from the cache system if one is configured
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
            error!("cache_get experienced an issue: {:?}", e);
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
    // 1) Guard: snapshot what we need (the Logger is a cheap channel clone),
    //    read the key from guest memory. The guard is dropped at the end of
    //    this block.
    let (module_name, els, cache, key) = {
        let guard = env.read().await;
        let env_data = guard.data();

        let cache = if let Some(c) = &env_data.cache {
            c
        } else {
            return Err(FunctionErrors::CacheDisabled);
        };

        let key = match safely_get_string_async(env, key_buf, key_buf_len).await {
            Ok(s) => s,
            Err(e) => {
                error!("{}: Key error in cache_get: {:?}", env_data.module.name, e);
                return Err(FunctionErrors::ParametersNotUtf8);
            }
        };

        (
            env_data.module.name.clone(),
            env_data.external_logging_system.clone(),
            cache.clone(),
            key,
        )
    };

    let namespace = module_name.clone();

    // 2) NO guard held: the await below suspends the guest's stack and the
    //    executor thread is free to run other guests. The timeout now
    //    actually cancels the in-flight work instead of just unblocking
    //    the thread.
    let result = match timeout(Duration::from_secs(5), cache.get(&namespace, &key)).await {
        Ok(result) => result,
        Err(_) => return Err(FunctionErrors::TimeoutElapsed),
    };

    // 3) Map the result exactly like the sync version did.
    match result {
        Ok(Some(value)) => {
            match safely_write_data_back_async(env, &value.as_bytes(), data_buffer, data_buffer_len)
                .await
            {
                Ok(x) => Ok(x),
                Err(e) => {
                    error!("{}: Data write error in cache_get: {:?}", module_name, e);
                    Err(e)
                }
            }
        }
        Ok(None) => Ok(0),
        Err(e) => {
            log_cache_error(&els, &module_name, e);
            Err(FunctionErrors::CacheDisabled)
        }
    }
}

/// Log a cache system error to the external logging system, exactly like
/// the sync version did.
fn log_cache_error(els: &Logger, module_name: &str, e: crate::cache::CacheError) {
    if let Err(e) = els.log_internal_message(
        Severity::Error,
        format!("Cache system error in [{}]: {:?}", module_name, e),
    ) {
        error!("Logging system is not working!!: {:?}", e);
    }
}
