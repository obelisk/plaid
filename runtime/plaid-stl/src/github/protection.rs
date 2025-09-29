use std::{collections::HashMap, fmt::Display};

use crate::{
    github::{CheckCodeownersParams, CodeownersStatus},
    PlaidFunctionError,
};

/// Get protection rules for a branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
pub fn get_branch_protection_rules(
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_branch_protection_rules);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("branch", branch.to_string());

    let request = serde_json::to_string(&params).unwrap();

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

/// Get protection rules (as in ruleset) for a branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
pub fn get_branch_protection_ruleset(
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_branch_protection_ruleset);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("branch", branch.to_string());

    let request = serde_json::to_string(&params).unwrap();

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

/// Update branch protection rule for a single branch
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `branch` - The name of the branch. Cannot contain wildcard characters.
/// * `body` - Body of the PUT request. See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#update-branch-protection
pub fn update_branch_protection_rule(
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
    body: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, update_branch_protection_rule);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("branch", branch.to_string());
    params.insert("body", body.to_string());

    let request = serde_json::to_string(&params).unwrap();

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

/// Create a GitHub deployment environment for a given repository.
///
/// Arguments:
/// * `owner` - The owner of the repository
/// * `repo` - The name of the repository
/// * `env_name` - The name of the environment to be created
pub fn create_environment_for_repo(
    owner: &str,
    repo: &str,
    env_name: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_environment_for_repo);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("env_name", env_name.to_string());

    let request =
        serde_json::to_string(&params).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        github_create_environment_for_repo(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

/// Create a deployment branch protection rule for a GitHub environment.
///
/// Arguments:
/// * `owner` - The owner of the repository
/// * `repo` - The name of the repository
/// * `env_name` - The name of the environment to be created
/// * `branch` - The branch from which a deployment can be triggered. This will be set in the deployment protection rules
pub fn create_deployment_branch_protection_rule(
    owner: &str,
    repo: &str,
    env_name: &str,
    branch: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_deployment_branch_protection_rule);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("env_name", env_name.to_string());
    params.insert("branch", branch.to_string());

    let request =
        serde_json::to_string(&params).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

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

/// Enforce signed commits (if `activated` is true) or turn it off (if `activated` is false) on a given branch of a given repo.
/// For more details, see https://docs.github.com/en/enterprise-cloud@latest/rest/branches/branch-protection?apiVersion=2022-11-28#create-commit-signature-protection
pub fn require_signed_commits(
    owner: impl Display,
    repo: impl Display,
    branch: impl Display,
    activated: bool,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, require_signed_commits);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("branch", branch.to_string());
    params.insert(
        "activated",
        if activated {
            "true".to_string()
        } else {
            "false".to_string()
        },
    );

    let params = serde_json::to_string(&params).unwrap();
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
    owner: &str,
    repo: &str,
    env_name: Option<&str>,
    secret_name: &str,
    secret: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, configure_secret);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    if let Some(env_name) = env_name {
        params.insert("env_name", env_name.to_string());
    }
    params.insert("secret_name", secret_name.to_string());
    params.insert("secret", secret.to_string());

    let request =
        serde_json::to_string(&params).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

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

    let params = serde_json::to_string(&params).unwrap();
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
