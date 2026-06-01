use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{
        CreateFileRequest, DeleteDeployKeyParams, FetchFileCustomMediaType, FetchFileRequest,
        GithubApiWrapper, RepositoryCollaborator, RepositoryCustomProperty, SbomResponse,
    },
    PlaidFunctionError,
};

#[derive(Deserialize, Serialize)]
pub struct RemoveUserFromRepoParams {
    pub user: String,
    pub repo: String,
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn remove_user_from_repo(client_id: impl Display, repo: &str, user: &str) -> Result<(), i32> {
    remove_user_from_repo_detailed(client_id, repo, user).map_err(|_| -4)
}

/// Remove a user from a repo
/// ## Arguments
///
/// * `repo` - The repo to remove the user from
/// * `user` - The user to remove from `repo`
pub fn remove_user_from_repo_detailed(
    client_id: impl Display,
    repo: &str,
    user: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_repo);
    }

    let params = RemoveUserFromRepoParams {
        user: user.to_string(),
        repo: repo.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res = unsafe {
        github_remove_user_from_repo(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Deserialize, Serialize)]
pub struct AddUserToRepoParams {
    pub user: String,
    pub repo: String,
    pub permission: Option<String>,
}

/// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_repo(
    client_id: impl Display,
    repo: &str,
    user: &str,
    permission: Option<&str>,
) -> Result<(), i32> {
    add_user_to_repo_detailed(client_id, repo, user, permission).map_err(|_| -4)
}

/// Add a user to a repo
/// ## Arguments
///
/// * `repo` - The repo to add the user to
/// * `user` - The user to add to `repo`
pub fn add_user_to_repo_detailed(
    client_id: impl Display,
    repo: &str,
    user: &str,
    permission: Option<&str>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_repo);
    }

    let params = AddUserToRepoParams {
        user: user.to_string(),
        repo: repo.to_string(),
        permission: permission.map(|p| p.to_string()),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res =
        unsafe { github_add_user_to_repo(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Deserialize, Serialize)]
pub struct GetRepoCollaboratorsParams {
    pub owner: String,
    pub repo: String,
    pub per_page: Option<u8>,
    pub page: Option<u16>,
    pub affiliation: Option<String>,
}

/// DEPRECATED - DO NOT USE. Instead, use get_all_repository_collaborators
/// Get first 30 collaborators on a repository
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_repository_collaborators(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repository_collaborators);
    }
    let params = GetRepoCollaboratorsParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        per_page: None,
        page: None,
        affiliation: None,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_repository_collaborators(
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
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    Ok(String::from_utf8(return_buffer).unwrap())
}

/// Get all collaborators on a repository.
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_all_repository_collaborators(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<Vec<RepositoryCollaborator>, PlaidFunctionError> {
    get_all_repository_collaborators_detailed(client_id, owner, repo, None)
}

/// Get all collaborators on a repository with direct access.
pub fn get_all_repository_collaborators_direct_access(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<Vec<RepositoryCollaborator>, PlaidFunctionError> {
    get_all_repository_collaborators_detailed(client_id, owner, repo, Some("direct"))
}

/// Get all collaborators on a repository with affiliation filter.
pub fn get_all_repository_collaborators_detailed(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    affiliation: Option<&str>,
) -> Result<Vec<RepositoryCollaborator>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repository_collaborators);
    }

    let mut collaborators = Vec::<RepositoryCollaborator>::new();
    let mut page = 0;

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    loop {
        page += 1;

        let params = GetRepoCollaboratorsParams {
            owner: owner.to_string(),
            repo: repo.to_string(),
            per_page: None,
            page: Some(page),
            affiliation: affiliation.map(|a| a.to_string()),
        };

        let wrapper = GithubApiWrapper {
            client_id: client_id.to_string(),
            params,
        };

        let request = serde_json::to_string(&wrapper).unwrap();

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_get_repository_collaborators(
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
        // This should be safe because unless the Plaid runtime is expressly trying
        // to mess with us, this came from a String in the API module.
        let this_page = String::from_utf8(return_buffer).unwrap();
        if this_page == "[]" {
            break;
        }
        collaborators.extend(
            serde_json::from_str::<Vec<RepositoryCollaborator>>(&this_page)
                .map_err(|_| PlaidFunctionError::InternalApiError)?,
        );
    }

    Ok(collaborators)
}

#[derive(Deserialize, Serialize)]
pub struct GetCustomPropertiesValuesParams {
    pub owner: String,
    pub repo: String,
}

/// Get custom properties for a repository
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_custom_properties_values(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<Vec<RepositoryCustomProperty>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_custom_properties_values);
    }

    let params = GetCustomPropertiesValuesParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
    };
    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };
    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_custom_properties_values(
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
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let response_body =
        String::from_utf8(return_buffer).map_err(|_| PlaidFunctionError::InternalApiError)?;
    let response_body = serde_json::from_str::<Vec<RepositoryCustomProperty>>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

#[derive(Deserialize, Serialize)]
pub struct GetRepoSbomParams {
    pub owner: String,
    pub repo: String,
}

/// Get the software bill of materials (SBOM) for a repository in SPDX JSON format.
pub fn get_repo_sbom(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<SbomResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repo_sbom);
    }

    let params = GetRepoSbomParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };
    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_repo_sbom(
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
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let response = String::from_utf8(return_buffer).unwrap();
    Ok(serde_json::from_str(&response).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?)
}

/// Gets the contents of a file or directory in a repository.
/// ## Arguments:
///
/// * `organization`: The account owner of the repository. The name is not case sensitive.
/// * `repository_name`: The name of the repository without the .git extension. The name is not case sensitive.
/// * `file_path`: Path of the file or directory to read
/// * `reference`: The name of the commit/branch/tag
pub fn fetch_file(
    client_id: impl Display,
    organization: &str,
    repository_name: &str,
    file_path: &str,
    reference: &str,
) -> Result<String, PlaidFunctionError> {
    fetch_file_with_custom_media_type(
        client_id,
        organization,
        repository_name,
        file_path,
        reference,
        FetchFileCustomMediaType::Default,
    )
}

/// Gets the contents of a file or directory in a repository.
/// ## Arguments:
///
/// * `organization`: The account owner of the repository. The name is not case sensitive.
/// * `repository_name`: The name of the repository without the .git extension. The name is not case sensitive.
/// * `file_path`: Path of the file or directory to read
/// * `reference`: The name of the commit/branch/tag
/// * `media_type`: The media type to fetch
pub fn fetch_file_with_custom_media_type(
    client_id: impl Display,
    organization: &str,
    repository_name: &str,
    file_path: &str,
    reference: &str,
    media_type: FetchFileCustomMediaType,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, fetch_file_with_custom_media_type);
    }
    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

    let request = FetchFileRequest {
        organization: organization.to_string(),
        repository_name: repository_name.to_string(),
        file_path: file_path.to_string(),
        reference: reference.to_string(),
        media_type,
    };
    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params: request,
    };
    let request = serde_json::to_string(&wrapper).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_fetch_file_with_custom_media_type(
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

#[derive(Serialize, Deserialize)]
pub struct FetchCommitParams {
    pub user: String,
    pub repo: String,
    pub commit: String,
}

/// Returns the contents of a single commit reference
/// ## Arguments
///
/// * `user` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `commit` - The commit reference. Can be a commit SHA, branch name (heads/BRANCH_NAME), or tag name (tags/TAG_NAME)
pub fn fetch_commit(
    client_id: impl Display,
    user: &str,
    repo: &str,
    commit: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, fetch_commit);
    }
    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

    let params = FetchCommitParams {
        user: user.to_string(),
        repo: repo.to_string(),
        commit: commit.to_string(),
    };
    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };
    let request = serde_json::to_string(&wrapper).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_fetch_commit(
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

/// Delete a deploy key with given ID from a given repository.
/// For more details, see https://docs.github.com/en/rest/deploy-keys/deploy-keys?apiVersion=2022-11-28#delete-a-deploy-key
pub fn delete_deploy_key(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    key_id: u64,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, delete_deploy_key);
    }

    let params = DeleteDeployKeyParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        key_id,
    };

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapped).unwrap();
    let res =
        unsafe { github_delete_deploy_key(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Creates a new file in a repository (create-only).
///
/// Creates a **new** file in the given `owner` and `repo` at the specified `path`.
/// The `message` will be used as the commit message. The `content` must be
/// provided as raw bytes (e.g., a UTF-8 string’s `.into()` or a `Vec<u8>`).
/// **Do not base64-encode the content** — this function will base64-encode it
/// automatically before sending it to the GitHub API.  
/// If `branch` is omitted, the repository’s default branch is used.
///
/// See the [GitHub API docs](https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#create-or-update-file-contents)
/// for protocol details (this function only supports creation; use the separate
/// update API to modify existing files).
///
/// # Arguments
/// - `owner`: The account or organization that owns the repository.
/// - `repo`: The name of the repository.
/// - `path`: The path, including filename, where the file will be created.
/// - `message`: The commit message to associate with the new file.
/// - `content`: The raw file contents (not base64-encoded).
/// - `branch`: The branch where the file will be created. Defaults to the
///   repository’s default branch if not provided.
///
/// # Returns
/// - `Ok(String)` with the hash of the created file if the request was successful, or
/// - `Err(PlaidFunctionError)` if the request fails (e.g., file already exists,
///   branch protection, missing configuration).
pub fn create_file(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    path: impl Display,
    message: impl Display,
    content: impl Into<Vec<u8>>,
    branch: Option<impl Display>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, create_file);
    }

    let request = CreateFileRequest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        path: path.to_string(),
        message: message.to_string(),
        content: content.into(),
        branch: branch.map(|b| b.to_string()),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params: request,
    };

    let request = serde_json::to_string(&wrapper).unwrap();
    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_create_file(
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

    Ok(response_body)
}

#[derive(Deserialize, Serialize)]
pub struct GetRepoIdFromRepoNameParams {
    pub owner: String,
    pub repo: String,
}

/// Get a repo ID from its name
/// ## Arguments
/// * `owner` - The owner of the repo.
/// * `repo` - The name of the repo without the owner.
pub fn get_repo_id_from_repo_name(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<i64, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repo_id_from_repo_name);
    }

    const RETURN_BUFFER_SIZE: usize = 32;
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = GetRepoIdFromRepoNameParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_get_repo_id_from_repo_name(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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
    let response = String::from_utf8(return_buffer).unwrap();
    let response: i64 = response.parse().unwrap();
    Ok(response)
}

#[derive(Deserialize, Serialize)]
pub struct GetRepoNameFromRepoIdParams {
    pub repo_id: String,
}

/// Get a repo_name from a repo ID
/// ## Arguments
/// * `repo_id` - The GitHub repo ID.
pub fn get_repo_name_from_repo_id(
    client_id: impl Display,
    repo_id: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repo_name_from_repo_id);
    }

    const RETURN_BUFFER_SIZE: usize = 64 * 1024; // 64 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = GetRepoNameFromRepoIdParams {
        repo_id: repo_id.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_get_repo_name_from_repo_id(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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
