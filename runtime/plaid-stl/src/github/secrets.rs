use std::fmt::Display;

use serde::Deserialize;

use crate::{
    github::{AddOrRemoveRepoToOrgSecretParams, GithubApiWrapper, ListOrgSecretsForRepoParams},
    PlaidFunctionError,
};

/// Add a repo to the list of repos that have access to an organization secret
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `org` - The organization name.
/// * `repository` - The repository name.
/// * `secret_name` - The name of the secret.
pub fn add_repo_to_org_secret(
    client_id: impl Display,
    org: impl Display,
    repository: impl Display,
    secret_name: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_repo_to_org_secret);
    }

    let params = AddOrRemoveRepoToOrgSecretParams {
        org: org.to_string(),
        repository: repository.to_string(),
        secret_name: secret_name.to_string(),
    };

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapped).unwrap();
    let res = unsafe {
        github_add_repo_to_org_secret(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Remove a repo from the list of repos that have access to an organization secret
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `org` - The organization name.
/// * `repository` - The repository name.
/// * `secret_name` - The name of the secret.
pub fn remove_repo_from_org_secret(
    client_id: impl Display,
    org: impl Display,
    repository: impl Display,
    secret_name: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_repo_from_org_secret);
    }

    let params = AddOrRemoveRepoToOrgSecretParams {
        org: org.to_string(),
        repository: repository.to_string(),
        secret_name: secret_name.to_string(),
    };

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };
    let params = serde_json::to_string(&wrapped).unwrap();
    let res = unsafe {
        github_remove_repo_from_org_secret(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// List the organization secrets that a repository has access to
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `org` - The organization name.
/// * `repository` - The repository name.
pub fn list_org_secrets_for_repo(
    client_id: impl Display,
    org: impl Display,
    repository: impl Display,
) -> Result<Vec<String>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_org_secrets_for_repo);
    }

    let mut page = 0;
    let mut secret_names = Vec::new();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    // Internal structs just for deserializing the response
    #[derive(Deserialize)]
    struct Secret {
        name: String,
    }

    #[derive(Deserialize)]
    struct SecretsPage {
        total_count: u32,
        secrets: Vec<Secret>,
    }

    loop {
        page += 1;
        let params = ListOrgSecretsForRepoParams {
            org: org.to_string(),
            repository: repository.to_string(),
            per_page: None,
            page: Some(page),
        };

        let wrapped = GithubApiWrapper {
            client_id: client_id.to_string(),
            params,
        };
        let params = serde_json::to_string(&wrapped).unwrap();
        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];
        let res = unsafe {
            github_list_org_secrets_for_repo(
                params.as_bytes().as_ptr(),
                params.as_bytes().len(),
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

        // Parse and process the page
        let this_page = serde_json::from_str::<SecretsPage>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;
        if this_page.secrets.is_empty() {
            break;
        }
        secret_names.extend(this_page.secrets.into_iter().map(|s| s.name));

        // Shortcut: if we've seen as many secrets as the total count (which is the same for every page), we know there are no more pages to fetch, so we can stop.
        // Keeping the other check around doesn't hurt either.
        if secret_names.len() as u32 >= this_page.total_count {
            break;
        }
    }

    Ok(secret_names)
}
