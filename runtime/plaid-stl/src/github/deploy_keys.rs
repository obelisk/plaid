use std::fmt::Display;

use crate::{
    github::{CreateDeployKeyParams, DeleteDeployKeyParams, GithubApiWrapper},
    PlaidFunctionError,
};

/// Delete a deploy key with given ID from a given repository.
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `owner` - The account owner of the repository.
/// * `repo` - The name of the repository.
/// * `key_id` - The ID of the deploy key to delete.
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

/// Create a deploy key for a given repository.
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `owner` - The account owner of the repository.
/// * `repo` - The name of the repository.
/// * `title` - The title of the deploy key.
/// * `key` - The deploy key.
/// * `read_only` - Whether the deploy key is read-only.
/// For more details, see https://docs.github.com/en/rest/deploy-keys/deploy-keys?apiVersion=2026-03-10#create-a-deploy-key
pub fn create_deploy_key(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    title: impl Display,
    key: impl Display,
    read_only: bool,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_deploy_key);
    }

    let params = CreateDeployKeyParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        title: title.to_string(),
        key: key.to_string(),
        read_only,
    };

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapped).unwrap();
    let res =
        unsafe { github_create_deploy_key(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
