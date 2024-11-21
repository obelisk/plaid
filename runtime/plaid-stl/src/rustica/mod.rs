use std::collections::HashMap;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 4 * 1024; // 4 KiB

pub fn new_mtls_cert(environment: &str, identity: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(rustica, new_mtls_cert);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("environment", environment.to_string());
    params.insert("identity", identity.to_string());
    let params = serde_json::to_string(&params).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        rustica_new_mtls_cert(
            params.as_ptr(),
            params.len(),
            return_buffer.as_mut_ptr(),
            return_buffer.len(),
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    Ok(String::from_utf8(return_buffer).unwrap())
}
