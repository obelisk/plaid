use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{github::GithubApiWrapper, PlaidFunctionError};

#[derive(Deserialize, Serialize)]
pub struct GrantRepoAccessToOrgInstallationParams {
    pub enterprise: String,
    pub org: String,
    pub installation_id: u64,
    pub repositories: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct RemoveRepoAccessFromOrgInstallationParams {
    pub enterprise: String,
    pub org: String,
    pub installation_id: u64,
    pub repositories: Vec<String>,
}

/// Grants a GitHub organization installation access to one or more repositories
/// ## Arguments
///
/// * `enterprise` - The enterprise name. The name is not case sensitive.
/// * `org` - The organization name. The name is not case sensitive.
/// * `installation_id` - The ID of the installation to grant access to.
/// * `repositories` - The list of repositories to grant access to.
pub fn grant_repo_access_to_org_installation(
    client_id: impl Display,
    enterprise: impl Display,
    org: impl Display,
    installation_id: u64,
    repositories: Vec<String>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, grant_repo_access_to_org_installation);
    }

    // Parse repo names and remove org if present
    let prefix = format!("{org}/");
    let repositories: Vec<String> = repositories
        .into_iter()
        .map(|repo| repo.trim_start_matches(&prefix).to_string())
        .collect();

    let params = GrantRepoAccessToOrgInstallationParams {
        enterprise: enterprise.to_string(),
        org: org.to_string(),
        installation_id,
        repositories,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_grant_repo_access_to_org_installation(
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

/// Removes a GitHub organization installation's access to one or more repositories
/// ## Arguments
///
/// * `enterprise` - The enterprise name. The name is not case sensitive.
/// * `org` - The organization name. The name is not case sensitive.
/// * `installation_id` - The ID of the installation to remove access from.
/// * `repositories` - The list of repositories to remove access from.
pub fn remove_repo_access_from_org_installation(
    client_id: impl Display,
    enterprise: impl Display,
    org: impl Display,
    installation_id: u64,
    repositories: Vec<String>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_repo_access_from_org_installation);
    }

    // Parse repo names and remove org if present
    let prefix = format!("{org}/");
    let repositories: Vec<String> = repositories
        .into_iter()
        .map(|repo| repo.trim_start_matches(&prefix).to_string())
        .collect();

    let params = RemoveRepoAccessFromOrgInstallationParams {
        enterprise: enterprise.to_string(),
        org: org.to_string(),
        installation_id,
        repositories,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_remove_repo_access_from_org_installation(
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
