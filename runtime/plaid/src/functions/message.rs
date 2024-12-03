use super::{get_memory, safely_get_string, safely_write_data_back};
use crate::executor::Env;

use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

pub fn fetch_data_and_source(
    env: FunctionEnvMut<Env>,
    data_buffer: WasmPtr<u8>,
    buffer_size: u32,
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
            return e as i32;
        }
    };

    let log_data = &env.data().message.data;

    // Get the from_module if it exists (which it won't if this is from a data generator
    // like GitHub, Okta, or a Webhook)
    let source = &env.data().message.source;

    // I really think this is overkill and we could just unwrap() this but
    // in the future we may run modules that are completely untrusted allowing things
    // like names to sneak in and perhaps cause issues. That is still a problem here
    // because this would then not succeed and the module will not know where a log came
    // from, but at least we can handle that.
    let source = match serde_json::to_vec(source) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "{}: Could not serialize the source: {}. Error: {e}",
                env.data().name,
                env.data().message.source,
            );
            return -4;
        }
    };

    // Get the length of the log and convert it to a byte representation
    let log_length = (log_data.len() as u32).to_le_bytes();

    let mut rule_data = Vec::new();
    rule_data.extend_from_slice(&log_length);
    rule_data.extend_from_slice(log_data);
    rule_data.extend_from_slice(&source);

    match safely_write_data_back(&memory_view, &rule_data, data_buffer, buffer_size) {
        Ok(x) => x,
        Err(e) => {
            error!("{}: Error in fetch_data: {:?}", env.data().name, e);
            e as i32
        }
    }
}

/// Wrap the fetch_data call in a native WASM function.
pub fn fetch_data(env: FunctionEnvMut<Env>, data_buffer: WasmPtr<u8>, buffer_size: u32) -> i32 {
    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_from_module: {:?}",
                env.data().name,
                e
            );
            return e as i32;
        }
    };

    let data = &env.data().message.data;

    match safely_write_data_back(&memory_view, data, data_buffer, buffer_size) {
        Ok(x) => x,
        Err(e) => {
            error!("{}: Error in fetch_data: {:?}", env.data().name, e);
            e as i32
        }
    }
}

/// Wrap the fetch_from_module call in a native WASM function.
pub fn fetch_source(env: FunctionEnvMut<Env>, data_buffer: WasmPtr<u8>, buffer_size: u32) -> i32 {
    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!(
                "{}: Memory error in fetch_from_module: {:?}",
                env.data().name,
                e
            );
            return e as i32;
        }
    };

    // Get the from_module if it exists (which it won't if this is from a data generator
    // like GitHub, Okta, or a Webhook)
    let source = &env.data().message.source;

    // I really think this is overkill and we could just unwrap() this but
    // in the future we may run modules that are completely untrusted allowing things
    // like names to sneak in and perhaps cause issues. That is still a problem here
    // because this would then not succeed and the module will not know where a log came
    // from, but at least we can handle that.
    let source = if let Ok(s) = serde_json::to_string(source) {
        s
    } else {
        error!(
            "{}: Could not serialize the source: {}",
            env.data().name,
            env.data().message.source,
        );
        return -4;
    };

    // Write the data back to the guest's memory
    match safely_write_data_back(&memory_view, source.as_bytes(), data_buffer, buffer_size) {
        Ok(x) => x,
        Err(e) => {
            error!("{}: Error in fetch_source: {:?}", env.data().name, e);
            e as i32
        }
    }
}

/// Wrap the fetch_accessory_data_by_name call in a native WASM function.
pub fn fetch_accessory_data_by_name(
    env: FunctionEnvMut<Env>,
    name_buf: WasmPtr<u8>,
    name_len: u32,
    data_buffer: WasmPtr<u8>,
    buffer_size: u32,
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
            return e as i32;
        }
    };

    let accessory_data = &env.data().message.accessory_data;

    let name = match safely_get_string(&memory_view, name_buf, name_len) {
        Ok(x) => x,
        Err(e) => {
            error!(
                "{}: Error in fetch_accessory_data_by_name: {:?}",
                env.data().name,
                e
            );
            return e as i32;
        }
    };

    // Check if this accessory data field is present at all
    if let Some(data) = accessory_data.get(&name) {
        match safely_write_data_back(&memory_view, data, data_buffer, buffer_size) {
            Ok(x) => x,
            Err(e) => {
                error!(
                    "{}: Error in fetch_accessory_data_by_name: {:?}",
                    env.data().name,
                    e
                );
                e as i32
            }
        }
    } else {
        // If there is no field with that name, we return 0 similar to
        // fetching the from_module
        0
    }
}
