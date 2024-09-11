use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

pub enum ReviewPatAction {
    Approve,
    Deny,
}

#[derive(Debug, Deserialize)]
/// Set of user permissions, as returned by GitHub's API
pub struct Permission {
    pub pull: bool,
    pub triage: bool,
    pub push: bool,
    pub maintain: bool,
    pub admin: bool,
}

#[derive(Debug, Deserialize)]
/// A collaborator on a GitHub repository
pub struct RepositoryCollaborator {
    pub login: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role_name: String,
    pub permissions: Permission,
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_team(team: &str, user: &str, org: &str, role: &str) -> Result<(), i32> {
    add_user_to_team_detailed(team, user, org, role).map_err(|_| -4)
}

/// Add a user to a team
/// ## Arguments
///
/// * `team` - The team to add the user to
/// * `user` - The user to add to `team`
/// * `org` - Github organization that `team` exists in
/// * `role` - Role to grant `user` on `team`
pub fn add_user_to_team_detailed(
    team: &str,
    user: &str,
    org: &str,
    role: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_team);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("team_slug", team);
    params.insert("org", org);
    params.insert("role", role);

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_user_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn remove_user_from_team(team: &str, user: &str, org: &str) -> Result<(), i32> {
    remove_user_from_team_detailed(team, user, org).map_err(|_| -4)
}

/// Remove a user from a team
/// ## Arguments
///
/// * `team` - The team to remove the user from
/// * `user` - The user to remove from `team`
/// * `org` - Github organization that `team` exists in
pub fn remove_user_from_team_detailed(
    team: &str,
    user: &str,
    org: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_team);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("team_slug", team);
    params.insert("org", org);

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_user_from_team(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn remove_user_from_repo(repo: &str, user: &str) -> Result<(), i32> {
    remove_user_from_repo_detailed(repo, user).map_err(|_| -4)
}

/// Remove a user from a repo
/// ## Arguments
///
/// * `repo` - The repo to remove the user from
/// * `user` - The user to remove from `repo`
pub fn remove_user_from_repo_detailed(repo: &str, user: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_repo);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("repo", repo);

    let params = serde_json::to_string(&params).unwrap();
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

/// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_repo(repo: &str, user: &str, permission: Option<&str>) -> Result<(), i32> {
    add_user_to_repo_detailed(repo, user, permission).map_err(|_| -4)
}

/// Add a user to a repo
/// ## Arguments
///
/// * `repo` - The repo to add the user to
/// * `user` - The user to add to `repo`
pub fn add_user_to_repo_detailed(
    repo: &str,
    user: &str,
    permission: Option<&str>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_repo);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("repo", repo);
    if let Some(permission) = permission {
        params.insert("permission", permission);
    }

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_user_to_repo(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn make_graphql_query(
    query_name: &str,
    variables: HashMap<String, String>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, String>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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

pub fn make_advanced_graphql_query(
    query_name: &str,
    variables: HashMap<String, serde_json::Value>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_advanced_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, serde_json::Value>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_advanced_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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

/// Returns the contents of a single commit reference
/// ## Arguments
///
/// * `user` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `commit` - The commit reference. Can be a commit SHA, branch name (heads/BRANCH_NAME), or tag name (tags/TAG_NAME)
pub fn fetch_commit(user: &str, repo: &str, commit: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, fetch_commit);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    #[derive(Serialize)]
    struct Request<'a> {
        user: &'a str,
        repo: &'a str,
        commit: &'a str,
    }

    let request = Request { user, repo, commit };

    let request = serde_json::to_string(&request).unwrap();

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

/// Lists approved fine-grained personal access tokens owned by organization members that can access organization resources
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
pub fn list_fpat_requests_for_org(org: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_fpat_requests_for_org);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("org", org);

    let request = serde_json::to_string(&params).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_list_fpat_requests_for_org(
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

/// Approves or denies multiple pending requests to access organization resources via a fine-grained personal access token
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
/// * `pat_request_ids` - Unique identifiers of the requests for access via fine-grained personal access token. Must be formed of between 1 and 100 pat_request_id values
/// * `action` - Action to apply to the requests.
/// * `reason` - Reason for approving or denying the requests. Max 1024 characters.
pub fn review_fpat_requests_for_org(
    org: String,
    pat_request_ids: &[u64],
    action: ReviewPatAction,
    reason: String,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, review_fpat_requests_for_org);
    }
    #[derive(Serialize)]
    struct Request {
        org: String,
        pat_request_ids: Vec<u64>,
        action: String,
        reason: String,
    }

    let request = Request {
        org,
        pat_request_ids: pat_request_ids.to_vec(),
        action: match action {
            ReviewPatAction::Approve => "approve".to_string(),
            ReviewPatAction::Deny => "deny".to_string(),
        },
        reason,
    };

    let request = serde_json::to_string(&request).unwrap();

    let res = unsafe {
        github_review_fpat_requests_for_org(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}

/// Lists the repositories a fine-grained personal access token request is requesting access to
/// ## Arguments
///
/// * `org` - The organization name. The name is not case sensitive.
/// * `request_id` - Unique identifier of the request for access via fine-grained personal access token.
/// * `per_page` - The number of results per page (max 100)
/// * `page` - The page number of the results to fetch.
pub fn get_repos_for_fpat<T: Display>(
    org: T,
    request_id: u64,
    per_page: Option<u64>,
    page: Option<u64>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repos_for_fpat);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("org", org.to_string());
    params.insert("request_id", request_id.to_string());
    if let Some(per_page) = per_page {
        params.insert("per_page", per_page.to_string());
    }
    if let Some(page) = page {
        params.insert("page", page.to_string());
    }

    let request = serde_json::to_string(&params).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_get_repos_for_fpat(
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

/// DEPRECATED - DO NOT USE. Instead, use get_all_repository_collaborators
/// Get first 30 collaborators on a repository
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_repository_collaborators(
    owner: impl Display,
    repo: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repository_collaborators);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());

    let request = serde_json::to_string(&params).unwrap();

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

/// Get all collaborators on a repository. Returns a vector of strings, where each string is a JSON-encoded
/// page of results, as returned by the GitHub API.
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_all_repository_collaborators(
    owner: impl Display,
    repo: impl Display,
) -> Result<Vec<RepositoryCollaborator>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repository_collaborators);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());

    let mut collaborators = Vec::<RepositoryCollaborator>::new();
    let mut page = 0;
    
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());
        // params.insert("per_page", "30".to_owned()"); // Default: 30 items per page
        
        let request = serde_json::to_string(&params).unwrap();
        
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
        collaborators.extend(serde_json::from_str::<Vec<RepositoryCollaborator>>(&this_page).map_err(|_| PlaidFunctionError::InternalApiError)?);
    }

    Ok(collaborators)
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
        new_host_function_with_error_buffer!(github, update_branch_protection_rule);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    params.insert("branch", branch.to_string());
    params.insert("body", body.to_string());

    let request = serde_json::to_string(&params).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_update_branch_protection_rule(
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

    Ok(())
}
