use super::errors::Errors;
use super::{LimitedAmount, LimitValue, LimitableAmount};

use std::collections::HashMap;
use std::fs::DirEntry;
use wasmer::wasmparser::Operator;

const CALL_COST: u64 = 10;

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

/// Get value for a limit, by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
fn get_limit_with_overrides(
    limit_amount: &LimitedAmount,
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

/// Get the computation limit for the module by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
pub fn get_module_computation_limit(
    limit_amount: &LimitedAmount,
    filename: &str,
    log_type: &str,
) -> u64 {
    get_limit_with_overrides(limit_amount, filename, log_type)
}

/// Get the persistent storage limit for the module by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
pub fn get_module_persistent_storage_limit(
    limit_amount: &LimitableAmount,
    filename: &str,
    log_type: &str,
) -> LimitValue {
    if let Some(amount) = limit_amount.module_overrides.get(filename) {
        amount.clone()
    } else if let Some(amount) = limit_amount.log_type.get(log_type) {
        amount.clone()
    } else {
        limit_amount.default.clone()
    }
}

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

/// Get the memory limit for the module by checking the following in order:
/// 1. Module Override
/// 2. Log Type amount
/// 3. Default amount
pub fn get_module_page_count(limit_amount: &LimitedAmount, filename: &str, log_type: &str) -> u32 {
    let page_count = get_limit_with_overrides(limit_amount, filename, log_type);

    // Page count is at max 32 bits. Nothing should ever allocate that many pages
    // but we're likely to hit this if someone spams the number key on their keyboard
    // for "unlimited memory".
    if page_count > u32::MAX as u64 {
        u32::MAX
    } else {
        page_count as u32
    }
}

/// Configures secrets for modules.
pub fn read_and_configure_secrets(
    secrets_configuration: &HashMap<String, HashMap<String, String>>,
) -> HashMap<String, HashMap<String, Vec<u8>>> {
    secrets_configuration
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                value
                    .into_iter()
                    .map(|(inner_key, inner_value)| {
                        (inner_key.to_string(), inner_value.as_bytes().to_vec())
                    })
                    .collect(),
            )
        })
        .collect()
}
