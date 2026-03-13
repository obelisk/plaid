mod api;
mod cache;
mod internal;
mod memory;
mod message;
mod response;
mod runtime_data;
mod storage;

use memory::*;

pub use api::is_known_api_function;
use api::to_api_function;
use wasmer::{Exports, Function, FunctionEnv, Module, Store};

use crate::executor::Env;

/// Errors that can be encountered during execution
#[derive(Debug)]
pub enum FunctionErrors {
    ApiNotConfigured = -1,
    ReturnBufferTooSmall = -2,
    ErrorCouldNotSerialize = -3,
    InternalApiError = -4,
    ParametersNotUtf8 = -5,
    InvalidPointer = -6,
    CacheDisabled = -7,
    CouldNotGetAdequateMemory = -8,
    FailedToWriteGuestMemory = -9,
    StorageLimitReached = -10,
    TestMode = -11,
    OperationNotAllowed = -12,
    SharedDbError = -13,
    TimeoutElapsed = -14,
}

#[derive(Debug)]
pub enum LinkError {
    NoSuchFunction(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::NoSuchFunction(name) => write!(f, "No such function: {}", name),
        }
    }
}

pub fn fake_wbindgen_describe(placeholder: i32) {
    warn!("Fake __wbindgen_describe called with placeholder: {placeholder}");
}

pub fn fake_wbindgen_throw(x: i32, y: i32) {
    warn!("Fake __wbindgen_throw called with placeholder: {x}, {y}");
}

pub fn fake_wbindgen_externref_table_grow(x: i32) -> i32 {
    warn!("Fake __wbindgen_externref_table_grow called with placeholder: {x}");
    return 0;
}

pub fn fake_wbindgen_externref_table_set_null(placeholder: i32) {
    warn!("Fake __wbindgen_externref_table_set_null called with placeholder: {placeholder}");
}

pub fn link_functions_to_module(
    module: &Module,
    mut store: &mut Store,
    env: FunctionEnv<Env>,
) -> Result<Exports, LinkError> {
    let mut exports = Exports::new();

    for import in module.imports() {
        let function_name = import.name();

        // Before 0.2.102, it's __wbingen*
        // From wasm-bindgen 0.2.102 to 0.2.104, it's __wbg_wbindgen*
        // From wasm-bindgen 0.2.105 onwards, it's __wbg___wbindgen*
        if function_name.starts_with("__wbindgen")
            || function_name.starts_with("__wbg_wbindgen")
            || function_name.starts_with("__wbg___wbindgen")
        {
            continue;
        }

        let func = to_api_function(function_name, &mut store, env.clone());
        if let Some(func) = func {
            exports.insert(function_name.to_string(), func);
            continue;
        }

        return Err(LinkError::NoSuchFunction(function_name.to_string()));
    }
    Ok(exports)
}

pub fn create_bindgen_placeholder(module: &Module, mut store: &mut Store) -> Exports {
    let mut exports = Exports::new();

    exports.insert(
        "__wbindgen_describe",
        Function::new_typed(&mut store, fake_wbindgen_describe),
    );

    exports.insert(
        "__wbindgen_throw",
        Function::new_typed(&mut store, fake_wbindgen_throw),
    );

    // wasm-bindgen >= 0.2.102 generates mangled import names for some intrinsics
    // including __wbindgen_throw
    for import in module.imports() {
        let name = import.name();
        // From wasm-bindgen 0.2.102 to 0.2.104, it's __wbg_wbindgenthrow_{hash}
        // From wasm-bindgen 0.2.105 onwards, it's __wbg___wbindgen_throw_{hash}
        if name.starts_with("__wbg_wbindgenthrow_") || name.starts_with("__wbg___wbindgen_throw_") {
            exports.insert(name, Function::new_typed(&mut store, fake_wbindgen_throw));
        }
    }

    exports
}

pub fn create_bindgen_externref_xform(mut store: &mut Store) -> Exports {
    let mut exports = Exports::new();

    exports.insert(
        "__wbindgen_externref_table_grow",
        Function::new_typed(&mut store, fake_wbindgen_externref_table_grow),
    );

    exports.insert(
        "__wbindgen_externref_table_set_null",
        Function::new_typed(&mut store, fake_wbindgen_externref_table_set_null),
    );

    exports
}
