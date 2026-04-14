use crate::{
    github::{InstallationAccessToken, InstallationAccessTokenRequest},
    PlaidFunctionError,
};

/// Create a GitHub App installation access token with an explicit scope and permission set.
pub fn create_installation_access_token(
    request: &InstallationAccessTokenRequest,
) -> Result<InstallationAccessToken, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, create_installation_access_token);
    }
    const RETURN_BUFFER_SIZE: usize = 64 * 1024;

    let request = serde_json::to_string(request).unwrap();
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_create_installation_access_token(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    let response = String::from_utf8(return_buffer).unwrap();
    serde_json::from_str(&response).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)
}
