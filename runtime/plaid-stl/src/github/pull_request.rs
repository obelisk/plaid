use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize};

use crate::{github::PullRequestRequestReviewers, PlaidFunctionError};

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

/// Request to fetch pull requests from a repository.
///
/// Represents the top-level parameters needed to query pull requests
/// for a given repository, including the repository owner, name, and
/// optional filter options.
#[derive(Serialize, Deserialize)]
pub struct GetPullRequestRequest {
    /// Owner of the repository (e.g., GitHub username or org).
    pub owner: String,
    /// Name of the repository.
    pub repo: String,
    /// Optional filter options to narrow down the pull request query.
    pub options: Option<GetPullRequestOptions>,
    /// Page number for pagination.
    pub page: u16,
    /// Number of results per page (max 100).
    pub per_page: usize,
}

/// Optional filters to apply when fetching pull requests.
///
/// Each field is optional; if none are provided, the request will
/// return all pull requests for the repository.
#[derive(Serialize, Deserialize, Default)]
pub struct GetPullRequestOptions {
    /// Filter pull requests by state (`open`, `closed`, or `all`).
    ///
    /// If not provided, GitHub defaults to `"open"`.
    pub state: Option<PullRequestState>,
    /// Filter pull requests by the source branch (the "head").
    ///
    /// Must be in the form `owner:branch`, where `owner` is the GitHub account
    /// that owns the repository containing the branch:
    ///   - If the PR branch lives in the same repository, `owner` is the same as
    ///     the repository owner you pass in the API path.
    ///   - If the PR branch comes from a fork, `owner` is the user or organization
    ///     that owns the fork.
    ///
    /// Examples:
    ///   - `octocat:feature-branch` (user-owned repo)
    ///   - `github:security-fix` (org-owned fork)
    ///
    /// Supplying only the branch name (without the `owner:` prefix) is not valid
    /// and will cause the filter to be ignored.
    pub head: Option<String>,
    /// Filter pull requests by the target branch (the "base") of the repository
    /// you are querying.
    pub base: Option<String>,
}

impl GetPullRequestOptions {
    /// Creates a new builder for constructing `GetPullRequestOptions`.
    pub fn builder() -> GetPullRequestOptionsBuilder {
        GetPullRequestOptionsBuilder::default()
    }
}

/// Builder for `GetPullRequestOptions`.
///
/// Provides a fluent interface for constructing `GetPullRequestOptions`
/// without having to manually set optional fields.
#[derive(Default)]
pub struct GetPullRequestOptionsBuilder {
    /// Filter pull requests by state (`open`, `closed`, or `all`).
    ///
    /// If not provided, GitHub defaults to `"open"`.
    pub state: Option<PullRequestState>,
    /// Filter pull requests by the source branch (the "head").
    ///
    /// Must be in the form `owner:branch`, where `owner` is the GitHub account
    /// that owns the repository containing the branch:
    ///   - If the PR branch lives in the same repository, `owner` is the same as
    ///     the repository owner you pass in the API path.
    ///   - If the PR branch comes from a fork, `owner` is the user or organization
    ///     that owns the fork.
    ///
    /// Examples:
    ///   - `octocat:feature-branch` (user-owned repo)
    ///   - `github:security-fix` (org-owned fork)
    ///
    /// Supplying only the branch name (without the `owner:` prefix) is not valid
    /// and will cause the filter to be ignored.
    pub head: Option<String>,
    /// Filter pull requests by the target branch (the "base") of the repository
    /// you are querying.
    pub base: Option<String>,
}

impl GetPullRequestOptionsBuilder {
    /// Sets the pull request state filter.
    pub fn state(mut self, state: PullRequestState) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the head branch filter.
    pub fn head<T: Into<String>>(mut self, head: T) -> Self {
        self.head = Some(head.into());
        self
    }

    /// Sets the base branch filter.
    pub fn base<T: Into<String>>(mut self, base: T) -> Self {
        self.base = Some(base.into());
        self
    }

    /// Consumes the builder and returns the constructed `GetPullRequestOptions`.
    pub fn build(self) -> GetPullRequestOptions {
        GetPullRequestOptions {
            state: self.state,
            head: self.head,
            base: self.base,
        }
    }
}

/// Possible states of a pull request.
#[derive(Serialize, Deserialize)]
pub enum PullRequestState {
    /// Only open pull requests.
    Open,
    /// Only closed pull requests.
    Closed,
    /// All pull requests, regardless of state.
    All,
}

impl Display for PullRequestState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "closed"),
            Self::All => write!(f, "all"),
        }
    }
}

/// Minimal representation of a GitHub Pull Request.
/// Captures identifiers, state, timestamps, author, and branch context.
#[derive(Debug, Deserialize, Clone)]
pub struct PullRequest {
    /// Unique internal GitHub ID for the pull request.
    pub id: i64,
    /// Pull request number within the repository.
    pub number: i64,
    /// API URL for this pull request.
    pub url: String,
    /// Web URL for viewing the pull request in the browser.
    pub html_url: String,
    /// Title of the pull request.
    pub title: String,
    /// Current state (e.g., "open", "closed").
    pub state: String,
    /// Whether the pull request is marked as a draft.
    pub draft: Option<bool>,
    /// ISO8601 timestamp if the PR was merged.
    pub merged_at: Option<String>,
    /// ISO8601 timestamp if the PR was closed.
    pub closed_at: Option<String>,
    /// ISO8601 timestamp when the PR was created.
    pub created_at: String,
    /// ISO8601 timestamp when the PR was last updated.
    pub updated_at: String,
    /// Author of the pull request, if present.
    pub user: Option<User>,
    /// The branch the changes are proposed from.
    pub head: BranchSummary,
    /// The branch the changes are proposed into.
    pub base: BranchSummary,
}

/// Minimal representation of a GitHub user.
/// Includes login name and numeric identifier.
#[derive(Debug, Deserialize, Clone)]
pub struct User {
    /// GitHub username (login).
    pub login: String,
    /// Unique internal GitHub user ID.
    pub id: i64,
}

/// Summary of a branch reference in a pull request.
/// Includes the branch name and repository context.
#[derive(Debug, Deserialize, Clone)]
pub struct BranchSummary {
    /// Branch name (ref).
    #[serde(rename = "ref")]
    pub r#ref: String,
    /// Repository the branch belongs to.
    pub repo: RepoSummary,
}

/// Summary of a repository.
/// Minimal set of fields useful for identifying and linking.
#[derive(Debug, Deserialize, Clone)]
pub struct RepoSummary {
    /// Full "owner/repo" name.
    pub full_name: String,
    /// Web URL for viewing the repository in the browser.
    pub html_url: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreatePullRequestRequest {
    /// The account owner of the repository. The name is not case sensitive.
    pub owner: String,
    /// The name of the repository without the `.git` extension. The name is not case sensitive.
    pub repo: String,
    /// The title of the new pull request
    pub title: String,
    /// The name of the branch where your changes are implemented.
    /// For cross-repository pull requests in the same network, namespace head
    /// with a user like this: username:branch.
    pub head: String,
    /// The name of the branch you want the changes pulled into. This should be an existing branch
    /// on the current repository. You cannot submit a pull request to one repository that
    /// requests a merge to a base of another repository.
    pub base: String,
    /// The contents of the pull request.
    pub body: Option<String>,
    /// Indicates whether the pull request is a draft
    pub draft: bool,
}

/// Fetches pull requests from a repository.
///
/// Queries the pull requests for the given `owner` and `repo`.  
/// Optional filters can be applied via `GetPullRequestOptions`, such as:
/// - limiting results to a particular state (`open`, `closed`, or `all`)
/// - filtering by source branch (`head`)
/// - filtering by target branch (`base`)
///
/// See the [GitHub API docs](https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28#list-pull-requests)
/// for more details.
///
/// # Arguments
/// - `owner`: The account or organization that owns the repository.
/// - `repo`: The name of the repository.
/// - `options`: Optional filters to narrow the set of returned pull requests.
///
/// # Returns
/// - `Ok(Vec<PullRequest>)` with all matching pull requests, or
/// - `Err(PlaidFunctionError)` if the request fails.
pub fn get_pull_requests(
    owner: impl Display,
    repo: impl Display,
    options: Option<GetPullRequestOptions>,
) -> Result<Vec<PullRequest>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_pull_requests);
    }

    let per_page = 100;
    let mut request_base = GetPullRequestRequest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        options,
        page: 1,
        per_page,
    };
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 5; // 5 MiB

    let mut prs = Vec::<PullRequest>::new();

    loop {
        let request = serde_json::to_string(&request_base).unwrap();

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];
        let res = unsafe {
            github_get_pull_requests(
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
        let this_page = serde_json::from_str::<Vec<PullRequest>>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;
        let prs_returned = this_page.len();

        prs.extend(this_page);

        // If we did not fill this page, we know there won't be a next one.
        if prs_returned < per_page {
            break;
        }

        request_base.page += 1;
    }

    Ok(prs)
}

/// Creates a pull request in a repository.
///
/// Opens a new pull request in the given `owner` and `repo`. The `title`
/// and `head`/`base` branches are required, with an optional `body` for
/// a description. Set `draft` to `true` to open the pull request as a draft.  
/// This function only supports creation; updating or merging pull requests
/// must be done via separate APIs.
///
/// See the [GitHub API docs](https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28#create-a-pull-request)
/// for more details.
///
/// # Arguments
/// - `owner`: The account or organization that owns the repository.
/// - `repo`: The name of the repository.
/// - `title`: The title of the pull request.
/// - `head`: The name of the branch where changes are implemented (the source branch).
/// - `base`: The name of the branch you want the changes pulled into (the target branch).
/// - `body`: Optional text providing a description of the pull request.
/// - `draft`: Whether to create the pull request as a draft (`true`) or a normal PR (`false`).
///
/// # Returns
/// - `Ok(())` if the pull request was successfully created, or
/// - `Err(PlaidFunctionError)` if the request fails (e.g., invalid branches,
///   missing permissions, or Plaid system misconfiguration).
pub fn create_pull_request(
    owner: impl Display,
    repo: impl Display,
    title: impl Display,
    head: impl Display,
    base: impl Display,
    body: Option<impl Display>,
    draft: bool,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, create_pull_request);
    }

    let request = CreatePullRequestRequest {
        owner: owner.to_string(),
        repo: repo.to_string(),
        title: title.to_string(),
        head: head.to_string(),
        base: base.to_string(),
        body: body.map(|b| b.to_string()),
        draft,
    };

    let request = serde_json::to_string(&request).unwrap();

    let res = unsafe {
        github_create_pull_request(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
