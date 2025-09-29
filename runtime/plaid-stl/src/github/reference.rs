use std::fmt::Display;

use crate::{
    github::{GetOrCreateBranchReferenceParams, GitApiRef, GitRef},
    PlaidFunctionError,
};

/// Returns a single Git reference (branch or tag) from the repository.
///
/// This function only requires the **short name** of the reference:
/// - For a branch, pass the branch name (e.g., `"main"`, not `"refs/heads/main"`).
/// - For a tag, pass the tag name (e.g., `"v1.0.0"`, not `"refs/tags/v1.0.0"`).
///
/// The API call will automatically expand these into fully qualified
/// Git reference paths under `refs/heads/` or `refs/tags/`.
///
/// See the [GitHub API docs](https://docs.github.com/en/rest/git/refs?apiVersion=2022-11-28#get-a-reference)
/// for more details.
///
/// # Arguments
/// * `owner` - The account owner of the repository. Case-insensitive.
/// * `repo` - The name of the repository without the `.git` extension. Case-insensitive.
/// * `reference` - A `GitRef` representing either a branch or a tag, specified by its short name.
///
/// # Returns
/// - `Ok(Some(GitApiRef))` if the reference exists.
/// - `Ok(None)` if the reference does not exist.
/// - `Err(PlaidFunctionError)` if the request fails.
pub fn get_reference(
    owner: impl Display,
    repo: impl Display,
    reference: GitRef,
) -> Result<Option<GitApiRef>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_reference);
    }

    let request = GetOrCreateBranchReferenceParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        reference,
        sha: None,
    };

    let request = serde_json::to_string(&request).unwrap();
    const RETURN_BUFFER_SIZE: usize = 1024 * 10; // 10 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_reference(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
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
    let response_body =
        String::from_utf8(return_buffer).map_err(|_| PlaidFunctionError::InternalApiError)?;

    if response_body.is_empty() {
        Ok(None)
    } else {
        let reference = serde_json::from_str::<GitApiRef>(&response_body)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        Ok(Some(reference))
    }
}

/// Creates a new Git reference (branch or tag) in the repository.
///
/// This function only requires the **short name** of the reference:
/// - For a branch, pass the branch name (e.g., `"feature-x"`, not `"refs/heads/feature-x"`).
/// - For a tag, pass the tag name (e.g., `"v1.0.0"`, not `"refs/tags/v1.0.0"`).
///
/// The API call will automatically expand these into fully qualified
/// Git reference paths under `refs/heads/` or `refs/tags/`.
///
/// See the [GitHub API docs](https://docs.github.com/en/rest/git/refs?apiVersion=2022-11-28#create-a-reference)
/// for more details.
///
/// # Arguments
/// * `owner` - The account owner of the repository. Case-insensitive.
/// * `repo` - The name of the repository without the `.git` extension. Case-insensitive.
/// * `reference` - A `GitRef` representing either a branch or a tag, specified by its short name.
/// * `sha` - The SHA-1 identifier of the commit or object the new reference should point to.
///
/// # Returns
/// - `Ok(())` if the reference was created successfully.
/// - `Err(PlaidFunctionError)` if the request fails.
pub fn create_reference(
    owner: impl Display,
    repo: impl Display,
    reference: GitRef,
    sha: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_reference);
    }

    let request = GetOrCreateBranchReferenceParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        reference,
        sha: Some(sha.to_string()),
    };

    let params = serde_json::to_string(&request).unwrap();
    let res =
        unsafe { github_create_reference(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
