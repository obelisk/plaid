use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

use crate::{datetime, PlaidFunctionError};

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

// Structs used for deserializing GH's responses when listing Copilot seats

#[derive(Debug, Deserialize)]
/// A person assigned to a seat in a Github Org Copilot subscription
pub struct CopilotAssignee {
    pub login: String,
    // type of assignee, such as "User"
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Deserialize)]
/// The team through which the assignee is granted access to GitHub Copilot
pub struct CopilotAssigningTeam {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Deserialize)]
/// A seat in a Github Org Copilot subscription
pub struct CopilotSeat {
    pub assignee: CopilotAssignee,
    pub plan_type: String,
    pub assigning_team: Option<CopilotAssigningTeam>,
    // Deserialize the date field from the YYYY-MM-DD format
    #[serde(deserialize_with = "datetime::deserialize_option_naivedate")]
    pub pending_cancellation_date: Option<NaiveDate>,
    // Deserialize the datetime field from the ISO 8601 format
    #[serde(deserialize_with = "datetime::deserialize_option_rfc3339_timestamp")]
    pub last_activity_at: Option<DateTime<Utc>>,
    // Deserialize the datetime field from the ISO 8601 format
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub created_at: DateTime<Utc>,
    // Deserialize the datetime field from the ISO 8601 format
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CopilotSeatsResult {
    pub total_seats: u64,
    pub seats: Vec<CopilotSeat>,
}

#[derive(Debug, Deserialize)]
pub struct CopilotAddUsersResponse {
    pub seats_created: u64,
}

#[derive(Debug, Deserialize)]
pub struct CopilotRemoveUsersResponse {
    pub seats_cancelled: u64,
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

/// Parameters sent to the runtime when triggering a GHA via repository_dispatch.
/// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#create-a-repository-dispatch-event for more details
#[derive(Serialize, Deserialize)]
pub struct RepositoryDispatchParams<T> {
    pub owner: String,
    pub repo: String,
    pub event_type: String,
    /// This is arbitrary content. We just need to be able to (de)serialize it.
    pub client_payload: T,
}

/// Parameters sent to the runtime when deleting a GH deploy key
#[derive(Serialize, Deserialize)]
pub struct DeleteDeployKeyParams {
    pub owner: String,
    pub repo: String,
    pub key_id: u64,
}

/// A custom property of a repo
/// See https://docs.github.com/en/organizations/managing-organization-settings/managing-custom-properties-for-repositories-in-your-organization for more details
#[derive(Debug, Deserialize)]
pub struct RepositoryCustomProperty {
    pub property_name: String,
    pub value: Option<String>,
}

/// Parameters sent to the runtime when checking a repo's CODEOWNERS file.
#[derive(Serialize, Deserialize)]
pub struct CheckCodeownersParams {
    pub owner: String,
    pub repo: String,
}

/// An error detected in a CODEOWNERS file.
/// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors for more details.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CodeownersError {
    pub kind: String,
    pub message: String,
    pub path: String,
    pub source: Option<String>,
}

/// Status for a repo's CODEOWNERS file
#[derive(Serialize, Deserialize, PartialEq)]
pub enum CodeownersStatus {
    /// The file is present and has no errors
    Ok,
    /// The file is missing
    Missing,
    /// The file is present but has at least one error
    Invalid(Vec<CodeownersError>),
}

impl Display for CodeownersStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::Ok => "Ok".to_string(),
            Self::Missing => "Missing".to_string(),
            Self::Invalid(errors) => format!("Invalid: {errors:?}"),
        };
        write!(f, "{str}")
    }
}

/// Response returned by GH API when checking for errors in a repo's CODEOWNERS file
#[derive(Debug, Deserialize)]
pub struct CodeownersErrorsResponse {
    pub errors: Vec<CodeownersError>,
}

#[derive(Deserialize)]
/// Number of commits made to a repo in a week
pub struct WeeklyCommits {
    pub all: Vec<u16>,
    pub owner: Vec<u16>,
}

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

#[derive(Serialize, Deserialize)]
pub struct GetOrCreateBranchReferenceParams {
    /// The account owner of the repository. The name is not case sensitive.
    pub owner: String,
    /// The name of the repository without the .git extension. The name is not case sensitive.
    pub repo: String,
    /// The Git reference. For more information, see [Git References](https://git-scm.com/book/en/v2/Git-Internals-Git-References) in the Git documentation.
    pub reference: GitRef,
    /// The SHA1 value for this reference.
    pub sha: Option<String>,
}

/// A reference in Git that points to a branch or a tag.
///
/// This is a simplified representation that only supports
/// `refs/heads/*` and `refs/tags/*`.
#[derive(Serialize, Deserialize)]
pub enum GitRef {
    /// A branch reference under `refs/heads/`
    Branch(String),
    /// A tag reference under `refs/tags/`
    Tag(String),
}

impl Display for GitRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tag(tag) => write!(f, "tags/{tag}"),
            Self::Branch(branch) => write!(f, "heads/{branch}"),
        }
    }
}

/// A Git reference as returned by the remote API.
///
/// Wraps an underlying object type (e.g., commit, tag)
/// and its associated SHA.
#[derive(Deserialize)]
pub struct GitApiRef {
    pub target: GitRefTarget,
}

/// The target object that a Git reference points to.
/// For example, a branch may point to a commit.
#[derive(Deserialize)]
pub struct GitRefTarget {
    /// The type of the object (e.g., "commit", "tag").
    #[serde(rename = "type")]
    pub type_: String,
    /// The SHA-1 hash of the referenced object.
    pub sha: String,
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

/// List seats in org's Copilot subscription, paginated
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `per_page` - The number of results per page (max 100)
/// * `page` - The page number of the results to fetch.
pub fn list_copilot_subscription_seats_by_page(
    org: &str,
    per_page: Option<u64>,
    page: Option<u64>,
) -> Result<Vec<CopilotSeat>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_seats_in_org_copilot);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("org", org.to_string());
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
        github_list_seats_in_org_copilot(
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
    let res = String::from_utf8(return_buffer).unwrap();

    let res = serde_json::from_str::<CopilotSeatsResult>(&res)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(res.seats)
}

/// List all seats in org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
pub fn list_all_copilot_subscription_seats(
    org: &str,
) -> Result<Vec<CopilotSeat>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_seats_in_org_copilot);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("org", org.to_string());
    // 100 is max per page
    params.insert("per_page", "100".to_string());

    let mut seats = Vec::<CopilotSeat>::new();
    let mut page = 0;

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());

        let request = serde_json::to_string(&params).unwrap();

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_list_seats_in_org_copilot(
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

        let this_page = serde_json::from_str::<CopilotSeatsResult>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        if this_page.seats.len() == 0 {
            break;
        }

        seats.extend(this_page.seats);
    }

    Ok(seats)
}

/// Add a user to the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `user` - The user to add to Copilot subscription
pub fn add_user_to_copilot_subscription(
    org: &str,
    user: &str,
) -> Result<CopilotAddUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, add_users_to_org_copilot);
    }
    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: vec![user],
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_add_users_to_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotAddUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

/// Remove a user from the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `user` - The user to remove from Copilot subscription
pub fn remove_user_from_copilot_subscription(
    org: &str,
    user: &str,
) -> Result<CopilotRemoveUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, remove_users_from_org_copilot);
    }

    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: vec![user],
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_users_from_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotRemoveUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

/// Remove multiple users from the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `users` - The list of users to remove from Copilot subscription
pub fn remove_users_from_copilot_subscription(
    org: &str,
    users: Vec<&str>,
) -> Result<CopilotRemoveUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, remove_users_from_org_copilot);
    }

    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: users,
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_users_from_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotRemoveUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
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
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

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
    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

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
    page: &str,
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
        page: &'a str,
    }

    let request = Request {
        organization,
        repository_name,
        pull_request,
        page,
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

/// Gets the contents of a file or directory in a repository.
/// ## Arguments:
///
/// * `organization`: The account owner of the repository. The name is not case sensitive.
/// * `repository_name`: The name of the repository without the .git extension. The name is not case sensitive.
/// * `file_path`: Path of the file or directory to read
/// * `reference`: The name of the commit/branch/tag
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

/// Get custom properties for a repository
/// ## Arguments
///
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
pub fn get_custom_properties_values(
    owner: impl Display,
    repo: impl Display,
) -> Result<Vec<RepositoryCustomProperty>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_custom_properties_values);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());

    let request = serde_json::to_string(&params).unwrap();

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

/// Search for code in GitHub.
/// If additional selection criteria are given, these are used to decide whether
/// results are selected or discarded.
///
/// **Arguments:**
/// - `filename`: The name of the files to search, e.g., "README"
/// - `extension`: The extension of the files to search, e.g., "yml"
/// - `path`: The path under which files are searched, e.g., "src"
/// - `search_criteria`: An optional `CodeSearchCriteria` object with additional search criteria
pub fn search_code(
    filename: Option<impl Display>,
    extension: Option<impl Display>,
    path: Option<impl Display>,
    search_criteria: Option<&CodeSearchCriteria>,
) -> Result<Vec<FileSearchResultItem>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, search_code);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    if let Some(filename) = filename {
        params.insert("filename", filename.to_string());
    }
    if let Some(extension) = extension {
        params.insert("extension", extension.to_string());
    }
    if let Some(path) = path {
        params.insert("path", path.to_string());
    }

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
    let per_page = 100;
    params.insert("per_page", per_page.to_string());

    const RETURN_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());

        let request = serde_json::to_string(&params).unwrap(); // safe unwrap

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_search_code(
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

        // Number of items we got on this page
        let received_page_size = file_search_result.items.len();

        search_results.extend(file_search_result.items);

        // If we did not fill this page, we know there won't be a next one.
        // So we can stop here and save one API call.
        if received_page_size < per_page {
            break;
        }
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

/// Trigger a GHA workflow via repository_dispatch.
/// For more details, see
/// * https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#create-a-repository-dispatch-event
/// * https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#repository_dispatch
pub fn trigger_repo_dispatch<T>(
    owner: &str,
    repo: &str,
    event_type: &str,
    client_payload: T,
) -> Result<(), PlaidFunctionError>
where
    T: Serialize + Deserialize<'static>,
{
    extern "C" {
        new_host_function!(github, trigger_repo_dispatch);
    }

    let params = RepositoryDispatchParams::<T> {
        owner: owner.to_string(),
        repo: repo.to_string(),
        event_type: event_type.to_string(),
        client_payload,
    };

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_trigger_repo_dispatch(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Check whether a user belongs to an org
/// ## Arguments
///
/// * `user` - The account to check org membership of
/// * `org` - The org to check if `user` is part of
pub fn check_org_membership_of_user(
    user: impl Display,
    org: impl Display,
) -> Result<bool, PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, check_org_membership_of_user);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("user", user.to_string());
    params.insert("org", org.to_string());

    let request = serde_json::to_string(&params).unwrap();

    let res = unsafe {
        github_check_org_membership_of_user(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    Ok(res != 0)
}

pub fn comment_on_pull_request(
    username: impl Display,
    repository_name: impl Display,
    pull_request: impl Display,
    comment: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, comment_on_pull_request);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("username", username.to_string());
    params.insert("repostory_name", repository_name.to_string());
    params.insert("pull_request", pull_request.to_string());
    params.insert("comment", comment.to_string());

    let request = serde_json::to_string(&params).unwrap();

    let res = unsafe {
        github_comment_on_pull_request(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Delete a deploy key with given ID from a given repository.
/// For more details, see https://docs.github.com/en/rest/deploy-keys/deploy-keys?apiVersion=2022-11-28#delete-a-deploy-key
pub fn delete_deploy_key(
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

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_delete_deploy_key(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Request reviewers for a pull request.
#[derive(Serialize, Deserialize)]
pub struct PullRequestRequestReviewers {
    pub owner: String,
    pub repo: String,
    pub pull_number: u64,
    pub reviewers: Vec<String>,
    pub team_reviewers: Vec<String>,
}

/// Request a reviewer on a PR
/// For more details, see https://docs.github.com/en/rest/pulls/review-requests?apiVersion=2022-11-28#request-reviewers-for-a-pull-request
pub fn pull_request_request_reviewers(
    owner: impl Display,
    repo: impl Display,
    pull_request: u64,
    reviewers: &[impl Display],
    team_reviewers: &[impl Display],
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, pull_request_request_reviewers);
    }

    let params = PullRequestRequestReviewers {
        owner: owner.to_string(),
        repo: repo.to_string(),
        pull_number: pull_request,
        reviewers: reviewers.iter().map(|s| s.to_string()).collect(),
        team_reviewers: team_reviewers.iter().map(|s| s.to_string()).collect(),
    };

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_pull_request_request_reviewers(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
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

/// Add a repo to a GH team, with a given permission.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#add-or-update-team-repository-permissions
pub fn add_repo_to_team(
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
    permission: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_repo_to_team);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("org", org.to_string());
    params.insert("team_slug", team_slug.to_string());
    params.insert("repo", repo.to_string());
    params.insert("permission", permission.to_string());

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_repo_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Get the weekly commit count on a given repo.
/// For more details, see https://docs.github.com/en/rest/metrics/statistics?apiVersion=2022-11-28#get-the-weekly-commit-count
pub fn get_weekly_commit_count(
    owner: impl Display,
    repo: impl Display,
) -> Result<WeeklyCommits, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_weekly_commit_count);
    }

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("owner", owner.to_string());
    params.insert("repo", repo.to_string());
    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe {
        github_get_weekly_commit_count(
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
    let response_body = serde_json::from_str::<WeeklyCommits>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

/// Remove a repo from a GH team.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#remove-a-repository-from-a-team
pub fn remove_repo_from_team(
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_repo_from_team);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("org", org.to_string());
    params.insert("team_slug", team_slug.to_string());
    params.insert("repo", repo.to_string());

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_repo_from_team(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
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
/// * `reference` - A [`GitRef`] representing either a branch or a tag, specified by its short name.
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
/// * `reference` - A [`GitRef`] representing either a branch or a tag, specified by its short name.
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
