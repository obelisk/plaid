/*******************************************************************************************
   SBOM
*******************************************************************************************/

use std::fmt::Display;

use crate::{datetime, PlaidFunctionError};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// The response returned by the GH API when asking for a repo's SBOM
#[derive(Deserialize)]
pub struct SbomResponse {
    pub sbom: Sbom,
}

/// A repo's SBOM
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sbom {
    pub spdx_version: String,
    pub name: String,
    pub document_namespace: String,
    pub packages: Vec<SbomPackage>,
    pub relationships: Option<Vec<Relationship>>,
}

/// An external reference, as returned as part of a repo's SBOM
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRef {
    pub reference_category: String,
    pub reference_locator: String,
    pub reference_type: String,
}

/// An SBOM package, as returned by the GH API
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SbomPackage {
    pub name: String,
    #[serde(rename = "SPXID")]
    pub spxid: Option<String>,
    pub version_info: String,
    pub files_analyzed: bool,
    pub download_location: String,
    pub license_concluded: Option<String>,
    pub copyright_text: Option<String>,
    pub external_refs: Option<Vec<ExternalRef>>,
}

/// A relationship with another item, as returned by the GH SBOM API
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub relationship_type: String,
    pub spdx_element_id: String,
    pub related_spdx_element: String,
}

/*******************************************************************************************
   COPILOT
*******************************************************************************************/

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

/*******************************************************************************************
   CODE SEARCH
*******************************************************************************************/

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
        let content = super::fetch_file(
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

/*******************************************************************************************
   CODEOWNERS
*******************************************************************************************/

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

/*******************************************************************************************
   GIT REFS
*******************************************************************************************/

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

/*******************************************************************************************
   MISCELLANEA
*******************************************************************************************/

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

#[derive(Deserialize)]
/// Number of commits made to a repo in a week
pub struct WeeklyCommits {
    pub all: Vec<u16>,
    pub owner: Vec<u16>,
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

/// Request to create a new file in a repository.
#[derive(Serialize, Deserialize)]
pub struct CreateFileRequest {
    /// Owner of the repository (e.g., GitHub username or org).
    pub owner: String,
    /// Name of the repository.
    pub repo: String,
    /// Path to create the file at
    pub path: String,
    /// The commit message.
    pub message: String,
    /// The new file content,
    pub content: Vec<u8>,
    /// The branch name. Default: the repository’s default branch.
    pub branch: Option<String>,
}
