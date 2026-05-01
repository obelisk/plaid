use std::fmt::Display;
use std::collections::HashMap;

use crate::PlaidFunctionError;

/// Get a user's ID from their username
/// ## Arguments
/// * `username` - The GitHub username.
pub fn get_user_id_from_username(username: impl Display) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_user_id_from_username);
    }

    const RETURN_BUFFER_SIZE: usize = 64 * 1024; // 64 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let username = username.to_string();

    let res = unsafe {
        github_get_user_id_from_username(
            username.as_bytes().as_ptr(),
            username.as_bytes().len(),
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

/// Get a username from a user ID
/// ## Arguments
/// * `user_id` - The GitHub user ID.
pub fn get_username_from_user_id(user_id: impl Display) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_username_from_user_id);
    }

    const RETURN_BUFFER_SIZE: usize = 64 * 1024; // 64 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let user_id = user_id.to_string();

    let res = unsafe {
        github_get_username_from_user_id(
            user_id.as_bytes().as_ptr(),
            user_id.as_bytes().len(),
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

/// Remove an outside collaborator from an org
/// ## Arguments
///
/// * `user` - The outside collaborator to remove from the org
/// * `org` - The GitHub organization to remove the user from
pub fn remove_outside_collaborator_from_org(
    user: impl Display,
    org: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_outside_collaborator_from_org);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("user", user.to_string());
    params.insert("org", org.to_string());

    let request = serde_json::to_string(&params).unwrap();

    let res = unsafe {
        github_remove_outside_collaborator_from_org(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
