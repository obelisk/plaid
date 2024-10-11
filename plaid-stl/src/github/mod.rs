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

// Structs used for deserializing GH's responses when searching for code

#[derive(Debug, Deserialize)]
pub struct FileSearchResult {
    pub total_count: u64,
    pub incomplete_results: bool,
    pub items: Vec<FileSearchResultItem>,
}

#[derive(Debug, Deserialize)]
pub struct FileSearchResultItem {
    pub path: String,
    pub html_url: String,
    pub url: String,
    pub repository: GithubRepository,
}

#[derive(Debug, Deserialize)]
pub struct GithubRepository {
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub description: Option<String>,
    pub owner: GithubRepositoryOwner,
}

#[derive(Debug, Deserialize)]
pub struct GithubRepositoryOwner {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubFileContent {
    #[serde(rename = "type")]
    pub type_: String,
    pub content: String,
    pub encoding: String,
}

// END OF Structs used for deserializing GH's response when searching for code

impl FileSearchResultItem {
    /// Retrieve the content of the search result
    pub fn retrieve_raw_content(&self) -> Result<String, PlaidFunctionError> {
        let reference_regex = regex::Regex::new(r"^.*?ref=([a-f0-9]{40})$").unwrap(); // TODO improve
        let reference = reference_regex
            .captures(&self.url)
            .ok_or(PlaidFunctionError::InternalApiError)?
            .get(1)
            .ok_or(PlaidFunctionError::InternalApiError)?
            .as_str();
        let content = fetch_file(
            &self.repository.owner.login,
            &self.repository.name,
            &self.path,
            reference,
        )
        .map_err(|_| PlaidFunctionError::InternalApiError)?;
        let content = serde_json::from_str::<GithubFileContent>(&content)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;
        if content.type_ != "file" || content.encoding != "base64" {
            return Err(PlaidFunctionError::InternalApiError); // TODO not the right error
        }
        // base64 decode and return the corresponding string
        Ok(String::from_utf8(
            base64::decode(content.content.replace("\n", ""))
                .map_err(|_| PlaidFunctionError::InternalApiError)?,
        )
        .map_err(|_| PlaidFunctionError::InternalApiError)?)
    }
}

/// Filter to specify whether to keep only results from a list of repositories,
/// or whether to discard all results that are from a list of repositories.
#[derive(Serialize, Deserialize)]
pub enum RepoFilter {
    OnlyFromRepos { repos: Vec<String> },
    NotFromRepos { repos: Vec<String> },
}

#[derive(Serialize, Deserialize)]
/// Specifies criteria according to which results returned
/// by the API should be included or discarded.
pub struct CodeSearchCriteria {
    /// Keep only results from this GH org
    pub only_from_org: Option<String>,
    /// Keep / discard results based on the repository name
    pub repo_filter: Option<RepoFilter>,
    /// Discard files where these strings appear in the file's path
    pub discard_substrings: Option<Vec<String>>,
    /// Discard files if some folder along their path is hidden (.folder)
    pub discard_results_in_dot_folders: bool,
    /// Discard files that belong to private repositories
    pub discard_results_in_private_repos: bool,
    /// Discard specific files identified as repository/path
    /// (e.g., myrepo/folderA/folderB/file.txt)
    pub discard_specific_files: Option<Vec<String>>,
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

pub fn list_files(
    organization: &str,
    repository_name: &str,
    pull_request: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_files);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    #[derive(Serialize)]
    struct Request<'a> {
        organization: &'a str,
        repository_name: &'a str,
        pull_request: &'a str,
    }

    let request = Request {
        organization,
        repository_name,
        pull_request,
    };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_list_files(
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

pub fn fetch_file(
    organization: &str,
    repository_name: &str,
    file_path: &str,
    reference: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, fetch_file);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    #[derive(Serialize)]
    struct Request<'a> {
        organization: &'a str,
        repository_name: &'a str,
        file_path: &'a str,
        reference: &'a str,
    }

    let request = Request {
        organization,
        repository_name,
        file_path,
        reference,
    };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_fetch_file(
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

/// Get all collaborators on a repository.
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
        collaborators.extend(
            serde_json::from_str::<Vec<RepositoryCollaborator>>(&this_page)
                .map_err(|_| PlaidFunctionError::InternalApiError)?,
        );
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

/// Search for files with given filename in GitHub.
/// If additional selection criteria are given, these are used to decide whether
/// results are selected or discarded.
///
/// **Arguments:**
/// - `filename`: The name of the files to search, e.g., "README.md"
/// - `search_criteria`: An optional `CodeSearchCriteria` object with additional search criteria
pub fn search_for_file(
    filename: impl Display,
    search_criteria: Option<&CodeSearchCriteria>,
) -> Result<Vec<FileSearchResultItem>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, search_for_file);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("filename", filename.to_string());

    // If we are given selection criteria, then we divide them between
    //
    // * Those that can be baked directly into the GitHub search query, thus making the overall search more
    // efficient (because less results are returned). These are passed to the API.
    //
    // * Those that have to be (or are better) evaluated module-side. These are not passed to the API and
    // are processed later here.

    if let Some(criteria) = search_criteria {
        if let Some(org) = &criteria.only_from_org {
            // Search only inside an organization
            params.insert("org", org.clone());

            if let Some(RepoFilter::OnlyFromRepos { repos }) = &criteria.repo_filter {
                if repos.len() == 1 {
                    // Special case: search only in a repository
                    params.insert("repo", repos[0].clone());
                }
            }
        }
    }

    let mut search_results = Vec::<FileSearchResultItem>::new();
    let mut page = 0;

    // Use a larger page size to make less requests and reduce chances of hitting the rate limit
    params.insert("per_page", "100".to_owned());

    const RETURN_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());

        let request = serde_json::to_string(&params).unwrap(); // safe unwrap

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_search_for_file(
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

        let file_search_result = serde_json::from_str::<FileSearchResult>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        if file_search_result.items.is_empty() {
            break; // we are past the last page
        }

        search_results.extend(file_search_result.items);
    }

    // Now that all the search results have been collected, apply the module-side selection criteria.
    if let Some(search_criteria) = search_criteria {
        Ok(filter_search_results(search_results, search_criteria))
    } else {
        // No criteria have been passed
        Ok(search_results)
    }
}

/// Filter results returned by GitHub search API by applying a set of search criteria
pub fn filter_search_results(
    raw_results: Vec<FileSearchResultItem>,
    search_criteria: &CodeSearchCriteria,
) -> Vec<FileSearchResultItem> {
    let mut filtered_results = Vec::<FileSearchResultItem>::new();
    let regex_dot_folder = regex::Regex::new(r"\/\.").unwrap(); // Right now, no way around recompiling this regex

    // Go through all the results and try to discard them by applying the criteria.
    // If the result makes it to the end, then add it to the filtered results.
    for result in raw_results {
        // Discard files in . folders
        if search_criteria.discard_results_in_dot_folders {
            if regex_dot_folder.is_match(&result.html_url) {
                continue;
            }
        }
        // Select / discard files based on the repo name. This _could_ be done in the query, but
        // there is a limit on how many AND / OR / NOT operators can be used. So we keep it here.
        if let Some(RepoFilter::NotFromRepos { repos }) = &search_criteria.repo_filter {
            if repos
                .iter()
                .find(|v| **v == result.repository.name)
                .is_some()
            {
                continue;
            }
        }
        if let Some(RepoFilter::OnlyFromRepos { repos }) = &search_criteria.repo_filter {
            if repos
                .iter()
                .find(|v| **v == result.repository.name)
                .is_none()
            {
                continue;
            }
        }
        // Discard files based on the repo's visibility
        if search_criteria.discard_results_in_private_repos && result.repository.private {
            continue;
        }
        // Discard files based on a substring in the path
        if let Some(sub_paths) = &search_criteria.discard_substrings {
            let mut discarded = false;
            for subp in sub_paths {
                if result.html_url.contains(subp) {
                    discarded = true;
                    break; // inner loop
                }
            }
            if discarded {
                continue;
            }
        }
        // Discard files based on explicit list
        if let Some(discard_explicit) = &search_criteria.discard_specific_files {
            // build the string we will search for
            let search = format!("{}/{}", result.repository.full_name, result.path);

            if discard_explicit.iter().find(|v| **v == search).is_some() {
                continue;
            }
        }

        // If we are here, we have not discarded the result
        filtered_results.push(result);
    }
    filtered_results
}
