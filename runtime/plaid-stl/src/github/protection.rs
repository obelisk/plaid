use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{CheckCodeownersParams, CodeownersStatus, ConfigureSecretParams, GithubApiWrapper},
    PlaidFunctionError,
};

#[derive(Serialize, Deserialize)]
pub struct GetBranchProtectionRulesParams {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

/// Get protection rules for a branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
pub fn get_branch_protection_rules(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_branch_protection_rules);
    }
    let params = GetBranchProtectionRulesParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_branch_protection_rules(
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

#[derive(Serialize, Deserialize)]
pub struct GetBranchProtectionRulesetParams {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

/// Get protection rules (as in ruleset) for a branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
pub fn get_branch_protection_ruleset(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_branch_protection_ruleset);
    }

    let params = GetBranchProtectionRulesetParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_branch_protection_ruleset(
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

#[derive(Serialize, Deserialize)]
pub struct UpdateBranchProtectionRuleParams {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub body: String,
}

/// Update branch protection rule for a single branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
/// * `body` - Body of the PUT request. See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#update-branch-protection
pub fn update_branch_protection_rule(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
    body: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, update_branch_protection_rule);
    }

    let params = UpdateBranchProtectionRuleParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
        body: body.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_update_branch_protection_rule(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct CreateEnvironmentForRepoParams {
    pub owner: String,
    pub repo: String,
    pub env_name: String,
}

/// Create a GitHub deployment environment for a given repository.
///
/// Arguments:
/// * `owner` - The owner of the repository
/// * `repo` - The name of the repository
/// * `env_name` - The name of the environment to be created
pub fn create_environment_for_repo(
    client_id: impl Display,
    owner: &str,
    repo: &str,
    env_name: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_environment_for_repo);
    }

    let params = CreateEnvironmentForRepoParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        env_name: env_name.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request =
        serde_json::to_string(&wrapper).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        github_create_environment_for_repo(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

#[derive(Serialize, Deserialize)]
pub struct CreateDeploymentBranchProtectionRuleParams {
    pub owner: String,
    pub repo: String,
    pub env_name: String,
    pub branch: String,
}

/// Create a deployment branch protection rule for a GitHub environment.
///
/// Arguments:
/// * `owner` - The owner of the repository
/// * `repo` - The name of the repository
/// * `env_name` - The name of the environment to be created
/// * `branch` - The branch from which a deployment can be triggered. This will be set in the deployment protection rules
pub fn create_deployment_branch_protection_rule(
    client_id: impl Display,
    owner: &str,
    repo: &str,
    env_name: &str,
    branch: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_deployment_branch_protection_rule);
    }

    let params = CreateDeploymentBranchProtectionRuleParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        env_name: env_name.to_string(),
        branch: branch.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request =
        serde_json::to_string(&wrapper).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        github_create_deployment_branch_protection_rule(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
        )
    };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

#[derive(Serialize, Deserialize)]
pub struct RequireSignedCommitsParams {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub activated: bool,
}

/// Enforce signed commits (if `activated` is true) or turn it off (if `activated` is false) on a given branch of a given repo.
/// For more details, see https://docs.github.com/en/enterprise-cloud@latest/rest/branches/branch-protection?apiVersion=2022-11-28#create-commit-signature-protection
pub fn require_signed_commits(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
    activated: bool,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, require_signed_commits);
    }

    let params = RequireSignedCommitsParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
        activated,
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res = unsafe {
        github_require_signed_commits(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Configure an environment secret for a GitHub deployment environment. If a secret with the same name is already present, it will be overwritten.
///
/// Arguments:
/// * `owner` - The owner of the repository
/// * `repo` - The name of the repository
/// * `env_name` - The name of the GitHub environment on which to set the secret. If not set, then a repository-level secret is created
/// * `secret_name` - The name of the secret to set
/// * `secret` - The plaintext secret to be set
pub fn configure_secret(
    client_id: impl Display,
    owner: &str,
    repo: &str,
    env_name: Option<&str>,
    secret_name: &str,
    secret: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, configure_secret);
    }

    let params = ConfigureSecretParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
        env_name: env_name.map(|s| s.to_string()),
        secret_name: secret_name.to_string(),
        secret: secret.to_string(),
    };

    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let request =
        serde_json::to_string(&wrapped).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res =
        unsafe { github_configure_secret(request.as_bytes().as_ptr(), request.as_bytes().len()) };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

/// Check a repo's CODEOWNERS file
/// For more details, see https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors
pub fn check_codeowners_file(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<CodeownersStatus, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, check_codeowners_file);
    }

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = CheckCodeownersParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
    };
    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res = unsafe {
        github_check_codeowners_file(
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
    let response_body =
        String::from_utf8(return_buffer).map_err(|_| PlaidFunctionError::InternalApiError)?;

    serde_json::from_str::<CodeownersStatus>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}
