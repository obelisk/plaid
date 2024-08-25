use std::time::{SystemTime, UNIX_EPOCH};

use plaid_stl::messages::LogbacksAllowed;
use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::{
    executor::Env,
    functions::{get_memory, safely_get_string},
};

use super::{safely_get_memory, safely_write_data_back, FunctionErrors};

/// Implement a way for a module to print to env_logger
pub fn print_debug_string(env: FunctionEnvMut<Env>, log_buffer: WasmPtr<u8>, log_buffer_size: u32) {
    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_from_module: {:?}",
                env.data().name,
                e
            );
            return;
        }
    };

    let message = match safely_get_string(&memory_view, log_buffer, log_buffer_size) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Error in print_debug_string: {:?}", env.data().name, e);
            return;
        }
    };

    debug!("Message from [{}]: {message}", env.data().name);
}

/// Implement a way for a module to get the existing response. This would have been
/// set by previous invocations of the module and allows an additional basic form of state.
pub fn get_response(
    env: FunctionEnvMut<Env>,
    response_buffer: WasmPtr<u8>,
    response_buffer_size: u32,
) -> i32 {
    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_from_module: {:?}",
                env.data().name,
                e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let response = match &env.data().response {
        Some(r) => r,
        None => {
            error!("{}: No response set", env.data().name);
            return 0;
        }
    };

    match safely_write_data_back(
        &memory_view,
        response.as_bytes(),
        response_buffer,
        response_buffer_size,
    ) {
        Ok(x) => x,
        Err(e) => {
            error!(
                "{}: Data write error in get_response: {:?}",
                env.data().name,
                e
            );
            e as i32
        }
    }
}

/// Implement a way for a module to set a response which is used for
/// get responses.
pub fn set_response(
    mut env: FunctionEnvMut<Env>,
    response_buffer: WasmPtr<u8>,
    response_buffer_size: u32,
) {
    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_from_module: {:?}",
                env.data().name,
                e
            );
            return;
        }
    };

    let message = match safely_get_string(&memory_view, response_buffer, response_buffer_size) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Error in set_response: {:?}", env.data().name, e);
            return;
        }
    };

    let mut env = env.as_mut();
    let data = env.data_mut();
    data.response = Some(message);
}

/// Implement a way for a module to get the current unixtime
pub fn get_time() -> u32 {
    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    return current_timestamp as u32;
}

/// Send a log from one module into the logging system to be picked up by another module
pub fn log_back(
    mut env: FunctionEnvMut<Env>,
    type_buf: WasmPtr<u8>,
    type_buf_len: u32,
    log_buf: WasmPtr<u8>,
    log_buf_len: u32,
    delay: u32,
    // How many logbacks the rule would like this new invocation to be able to trigger
    logbacks_requested: u32,
) -> u32 {
    let name = env.data().name.clone();
    // We need to check that the that the module has the logbacks_allowed "budget"
    // for the logback they are requesting
    let assigned_budget = match &mut env.data_mut().message.logbacks_allowed {
        // This message has unlimited budget meaning it can assign as much budget
        // as it desires.
        LogbacksAllowed::Unlimited => logbacks_requested,
        // There is no logback budget left so this call is not allowed to continue
        LogbacksAllowed::Limited(0) => {
            error!("{name}: Logback attempted with zero budget.");
            return 1;
        }
        LogbacksAllowed::Limited(x) => {
            // If more logbacks than the available budget is attempted
            // to be assigned then the call is not allowed to continue.
            //
            // We need to subtract one here because this logback itself costs
            // one. We also know this is safe because we've already checked that
            // the budget is not 0 so it will not underflow.
            if logbacks_requested > (*x - 1) {
                error!("{name}: Logback budget exceeded. Requested {logbacks_requested}, but only {x} was available.");
                return 1;
            }
            // The assigned logbacks are subtracted from the budget after
            // we've checked that it is smaller.
            *x -= logbacks_requested;
            *x -= 1; // Subtract the one that this logback costs

            logbacks_requested
        }
    };

    let store = env.as_store_ref();
    let env_data = env.data();

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in log_back: {:?}", env_data.name, e);
            return 1;
        }
    };

    let type_ = match safely_get_string(&memory_view, type_buf, type_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Error in log_back: {:?}", env_data.name, e);
            return 1;
        }
    };

    // Safely get the data from the guest's memory
    let log = match safely_get_memory(&memory_view, log_buf, log_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!("{}: Error in log_back: {:?}", env_data.name, e);
            return 1;
        }
    };

    let api = env_data.api.clone();
    api.clone().runtime.block_on(async move {
        match api.general.as_ref() {
            Some(general) => {
                if general.log_back(
                    &type_,
                    &log,
                    &env_data.name,
                    delay as u64,
                    LogbacksAllowed::Limited(assigned_budget),
                ) {
                    0
                } else {
                    1
                }
            }
            _ => 1,
        }
    })
}

/// Store data in the storage system if one is configured
pub fn storage_insert(
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

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    // Get the storage data from the client's memory
    let value = match safely_get_memory(&memory_view, value_buf, value_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!("{}: Value error in storage_insert: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let storage_key = key.clone();
    let result = env_data.api.clone().runtime.block_on(async move {
        storage
            .insert(env_data.name.clone(), storage_key, value)
            .await
    });

    match result {
        Ok(Some(data)) => {
            // If the data is too large to fit in the buffer that was passed to us. Unfortunately this is a somewhat
            // unrecoverable state because we've overwritten the value already. We could fail insertion if the data
            // buffer passed is too small in future? That would mean doing a get call first, which the client can do
            // too.
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_insert: {:?}",
                        env_data.name, e
                    );
                    e as i32
                }
            }
        }
        // This occurs when there is no such key so the number of bytes that have been copied back are 0
        Ok(None) => 0,
        // If the storage system errors (for example a network problem if using a networked storage provider)
        // the error is made opaque to the client here and we log what happened
        Err(e) => {
            error!(
                "There was a storage system error when key [{key}] was accessed by [{}]: {e}",
                env_data.name
            );
            return FunctionErrors::InternalApiError as i32;
        }
    }
}

/// Store data in the storage system if one is configured
pub fn storage_get(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let storage = if let Some(storage) = &env_data.storage {
        storage
    } else {
        return FunctionErrors::ApiNotConfigured as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in storage_get: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in storage_get: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };
    let result = env_data
        .api
        .clone()
        .runtime
        .block_on(async move { storage.get(&env_data.name, &key).await });

    match result {
        Ok(Some(data)) => {
            match safely_write_data_back(&memory_view, &data, data_buffer, data_buffer_len) {
                Ok(x) => x,
                Err(e) => {
                    error!(
                        "{}: Data write error in storage_get: {:?}",
                        env_data.name, e
                    );
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(_) => 0,
    }
}

/// Store data in the cache system if one is configured
pub fn cache_insert(
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

    let cache = if let Some(c) = &env_data.cache {
        c
    } else {
        return FunctionErrors::CacheDisabled as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in cache_insert: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in cache_insert: {:?}", env_data.name, e);
            return FunctionErrors::ParametersNotUtf8 as i32;
        }
    };

    // Get the storage data from the client's memory
    let value = match safely_get_string(&memory_view, value_buf, value_buf_len) {
        Ok(d) => d,
        Err(e) => {
            error!("{}: Value error in cache_insert: {:?}", env_data.name, e);
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
                        env_data.name, e
                    );
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(e) => {
            if let Err(e) = env_data.external_logging_system.log_internal_message(
                crate::logging::Severity::Error,
                format!("Cache system error in [{}]: {:?}", env_data.name, e),
            ) {
                error!("Logging system is not working!!: {:?}", e);
            }
            FunctionErrors::CacheDisabled as i32
        }
    }
}

/// Store data in the cache system if one is configured
pub fn cache_get(
    env: FunctionEnvMut<Env>,
    key_buf: WasmPtr<u8>,
    key_buf_len: u32,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u32,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let cache = if let Some(c) = &env_data.cache {
        c
    } else {
        return FunctionErrors::CacheDisabled as i32;
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{}: Memory error in cache_get: {:?}", env_data.name, e);
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    let key = match safely_get_string(&memory_view, key_buf, key_buf_len) {
        Ok(s) => s,
        Err(e) => {
            error!("{}: Key error in cache_get: {:?}", env_data.name, e);
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
                    error!("{}: Data write error in cache_get: {:?}", env_data.name, e);
                    e as i32
                }
            }
        }
        Ok(None) => 0,
        Err(e) => {
            if let Err(e) = env_data.external_logging_system.log_internal_message(
                crate::logging::Severity::Error,
                format!("Cache system error in [{}]: {:?}", env_data.name, e),
            ) {
                error!("Logging system is not working!!: {:?}", e);
            }
            FunctionErrors::CacheDisabled as i32
        }
    }
}

/// Implement a way for randomness to get into the module
pub fn fetch_random_bytes(
    env: FunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    data_buffer_len: u16,
) -> i32 {
    let store = env.as_store_ref();
    let env_data = env.data();

    let api = match env_data.api.general.as_ref() {
        Some(api) => api,
        None => {
            error!("General API not configured");
            return FunctionErrors::ApiNotConfigured as i32;
        }
    };

    let bytes: Vec<u8> = match api.fetch_random_bytes(data_buffer_len) {
        Ok(b) => b,
        Err(e) => {
            error!("Error fetching random bytes: {:?}", e);
            return FunctionErrors::InternalApiError as i32;
        }
    };

    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_random_bytes: {:?}",
                env_data.name, e
            );
            return FunctionErrors::CouldNotGetAdequateMemory as i32;
        }
    };

    match safely_write_data_back(&memory_view, &bytes, data_buffer, data_buffer_len as u32) {
        Ok(x) => x,
        Err(e) => {
            error!(
                "{}: Data write error in fetch_random_bytes: {:?}",
                env_data.name, e
            );
            e as i32
        }
    }
}
