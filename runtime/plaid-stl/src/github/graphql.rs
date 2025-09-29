use std::collections::HashMap;

use serde::Serialize;

use crate::PlaidFunctionError;

pub fn make_graphql_query(
    query_name: &str,
    variables: HashMap<String, String>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, String>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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

pub fn make_advanced_graphql_query(
    query_name: &str,
    variables: HashMap<String, serde_json::Value>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_advanced_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, serde_json::Value>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_advanced_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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
