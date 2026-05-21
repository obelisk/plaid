use std::fmt::Display;

use crate::{github::AddOrRemoveRepoToOrgSecretParams, PlaidFunctionError};

/// Add a repo to the list of repos that have access to an organization secret
pub fn add_repo_to_org_secret(
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
    let params = serde_json::to_string(&params).unwrap();
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
pub fn remove_repo_from_org_secret(
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
    let params = serde_json::to_string(&params).unwrap();
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
