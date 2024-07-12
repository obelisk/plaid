use super::errors::Errors;
use super::limits::LimitingTunables;
use super::LimitAmount;

use std::collections::HashMap;
use std::fs::DirEntry;
use std::sync::Arc;
use wasmer::{sys::BaseTunables, Engine, NativeEngineExt, Pages, Target};
use wasmer::{wasmparser::Operator, CompilerConfig, Cranelift, Module};
use wasmer_middlewares::Metering;

const CALL_COST: u64 = 10;

/// Returns the cost associated with a given WebAssembly operator.
///
/// This function is used as part of the Wasmer middleware to determine the computational
/// cost of executing various WebAssembly operators. It assigns a specific cost to call-related
/// operators and a default cost to all other operators.
pub fn cost_function(operator: &Operator) -> u64 {
    match operator {
        Operator::Call { .. } => CALL_COST,
        Operator::CallIndirect { .. } => CALL_COST,
        Operator::ReturnCall { .. } => CALL_COST,
        Operator::ReturnCallIndirect { .. } => CALL_COST,
        _ => 1,
    }
}

/// Get the module file name and read in the bytes
pub fn read_and_parse_modules(path: &DirEntry) -> Result<(String, Vec<u8>), Errors> {
    // Path's can be weird so we just try to make it a UTF8 string,
    // if it's not UTF8, we'll fail reading it and skip it.
    let filename = path.file_name().to_string_lossy().to_string();

    // Also skip any files that aren't wasm files
    if !filename.ends_with(".wasm") {
        return Err(Errors::BadFilename);
    }

    // Read in the bytes of the module
    let module_bytes = match std::fs::read(path.path()) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read module at [{:?}]. Error: {e}", path.path());
            return Err(Errors::ModuleParseFailure);
        }
    };

    Ok((filename, module_bytes))
}

/// Get the computation limit for the module by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
pub fn get_module_computation_limit(
    limit_amount: &LimitAmount,
    filename: &str,
    log_type: &str,
) -> u64 {
    if let Some(amount) = limit_amount.module_overrides.get(filename) {
        *amount
    } else if let Some(amount) = limit_amount.log_type.get(log_type) {
        *amount
    } else {
        limit_amount.default
    }
}

/// Get the memory limit for the module by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
pub fn get_module_page_count(limit_amount: &LimitAmount, filename: &str, log_type: &str) -> u32 {
    let page_count = if let Some(amount) = limit_amount.module_overrides.get(filename) {
        *amount
    } else if let Some(amount) = limit_amount.log_type.get(log_type) {
        *amount
    } else {
        limit_amount.default
    };

    // Page count is at max 32 bits. Nothing should ever allocate that many pages
    // but we're likely to hit this if someone spams the number key on their keyboard
    // for "unlimited memory".
    if page_count > u32::MAX as u64 {
        u32::MAX
    } else {
        page_count as u32
    }
}

/// Configure and compile a module with specified computation limits and memory page count.
///
/// This function sets up the computation metering, configures the module tunables, and
/// compiles the module using the provided bytecode and settings.
pub fn configure_and_compile_module(
    computation_limit: u64,
    page_count: u32,
    module_bytes: Vec<u8>,
    filename: &str,
) -> Result<(Module, Engine), Errors> {
    let metering = Arc::new(Metering::new(computation_limit, cost_function));
    let mut compiler = Cranelift::default();
    compiler.push_middleware(metering);

    // Configure module tunables - this includes our computation limit and page count
    let base = BaseTunables::for_target(&Target::default());
    let tunables = LimitingTunables::new(base, Pages(page_count));
    let mut engine: Engine = compiler.into();
    engine.set_tunables(tunables);

    // Compile the module using the middleware and tunables we just set up
    let mut module = Module::new(&engine, module_bytes).map_err(|e| {
        error!("Failed to compile module [{filename}]. Error: {e}");
        Errors::ModuleCompilationFailure
    })?;
    module.set_name(&filename);

    Ok((module, engine))
}

/// Reads and configures secrets for modules.
///
/// This function takes a `Map<String, Value>` representing the secrets read from a file,
/// and a `HashMap<String, HashMap<String, String>>` representing the secrets configuration.
/// The secrets configuration maps a user-defined secret name to the actual secret name in the read file.
/// It returns a `HashMap<String, HashMap<String, Vec<u8>>>` where the secrets have been
/// configured according to the provided configuration.
///
/// This setup allows for configuration files to be checked in, referencing secret names
/// that are mapped to actual secret values stored in the secrets file.
pub fn read_and_configure_secrets(
    secrets_configuration: HashMap<String, HashMap<String, String>>,
) -> HashMap<String, HashMap<String, Vec<u8>>> {
    secrets_configuration
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                value
                    .into_iter()
                    .map(|(inner_key, inner_value)| (inner_key, inner_value.as_bytes().to_vec()))
                    .collect(),
            )
        })
        .collect()
}
