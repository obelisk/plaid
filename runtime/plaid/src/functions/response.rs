use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::{executor::Env, functions::FunctionErrors};

use super::{get_memory, safely_get_string, safely_write_data_back};

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
