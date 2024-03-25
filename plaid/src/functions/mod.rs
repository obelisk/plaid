mod api;
mod internal;
mod memory;
mod message;

use memory::*;

use api::to_api_function;
use wasmer::{Exports, FunctionEnv, Module, Store};

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

pub fn link_functions_to_module(module: &Module, mut store: &mut Store, env: FunctionEnv<Env>) -> Result<Exports, LinkError>{
    let mut exports = Exports::new();

    for import in module.imports() {
        let function_name = import.name();
        let func = to_api_function(function_name, &mut store, env.clone());
        if let Some(func) = func {
            exports.insert(function_name.to_string(), func);
            continue;
        }

        return Err(LinkError::NoSuchFunction(function_name.to_string()));
    } 
    Ok(exports)
}