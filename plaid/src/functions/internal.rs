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

/// Implement a way for a module to get the current unixtime
pub fn get_time() -> u32 {
    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    return current_timestamp as u32;
}

/// Send a log back with a requested budget
pub fn log_back(
    env: FunctionEnvMut<Env>,
    type_buf: WasmPtr<u8>,
    type_buf_len: u32,
    log_buf: WasmPtr<u8>,
    log_buf_len: u32,
    delay: u32,
    // How many logbacks the rule would like this new invocation to be able to trigger
    logbacks_requested: u32,
) -> u32 {
    log_back_detailed(
        env,
        type_buf,
        type_buf_len,
        log_buf,
        log_buf_len,
        delay,
        LogbacksAllowed::Limited(logbacks_requested),
    )
}

/// Send a log back with unlimited budget
pub fn log_back_unlimited(
    env: FunctionEnvMut<Env>,
    type_buf: WasmPtr<u8>,
    type_buf_len: u32,
    log_buf: WasmPtr<u8>,
    log_buf_len: u32,
    delay: u32,
) -> u32 {
    log_back_detailed(
        env,
        type_buf,
        type_buf_len,
        log_buf,
        log_buf_len,
        delay,
        LogbacksAllowed::Unlimited,
    )
}

/// Send a log from one module into the logging system to be picked up by another module
pub fn log_back_detailed(
    mut env: FunctionEnvMut<Env>,
    type_buf: WasmPtr<u8>,
    type_buf_len: u32,
    log_buf: WasmPtr<u8>,
    log_buf_len: u32,
    delay: u32,
    // How many logbacks the rule would like this new invocation to be able to trigger
    logbacks_requested: LogbacksAllowed,
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
            // See what the caller was asking for
            match logbacks_requested {
                LogbacksAllowed::Limited(asked) => {
                    // If more logbacks than the available budget is attempted
                    // to be assigned then the call is not allowed to continue.
                    //
                    // We need to subtract one here because this logback itself costs
                    // one. We also know this is safe because we've already checked that
                    // the budget is not 0 so it will not underflow.
                    if asked > (*x - 1) {
                        error!("{name}: Logback budget exceeded. Requested {asked}, but only {x} was available.");
                        return 1;
                    }
                    // The assigned logbacks are subtracted from the budget after
                    // we've checked that it is smaller.
                    *x -= asked;
                    *x -= 1; // Subtract the one that this logback costs
                    logbacks_requested
                }
                LogbacksAllowed::Unlimited => {
                    error!("{name} attempted unlimited log back with limited budget. The budget was {x}");
                    return 1;
                }
            }
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
                if general.log_back(&type_, &log, &env_data.name, delay as u64, assigned_budget) {
                    0
                } else {
                    1
                }
            }
            _ => 1,
        }
    })
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
