use std::collections::HashMap;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 128 * 1024;

pub fn get_user_data(query: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(okta, get_user_data);
    }

    let query = query.as_bytes().to_vec();
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE]; // 16 KiB

    let res = unsafe {
        okta_get_user_data(
            query.as_ptr(),
            query.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
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

pub fn remove_user_from_group(user_id: &str, group_id: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(okta, remove_user_from_group);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user_id", user_id);
    params.insert("group_id", group_id);

    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { okta_remove_user_from_group(params.as_ptr(), params.len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    match res {
        0 => Ok(()),
        _ => Err(PlaidFunctionError::InternalApiError),
    }
}
