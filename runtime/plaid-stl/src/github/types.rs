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
    pub id: u64,
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
    /// ## Arguments
    ///
    /// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
    pub fn retrieve_raw_content(
        &self,
        client_id: impl Display,
    ) -> Result<String, PlaidFunctionError> {
        let reference_regex = regex::Regex::new(r"^.*?ref=([a-f0-9]{40})$").unwrap(); // TODO improve
        let reference = reference_regex
            .captures(&self.url)
            .ok_or(PlaidFunctionError::InternalApiError)?
            .get(1)
            .ok_or(PlaidFunctionError::InternalApiError)?
            .as_str();
        let content = super::fetch_file(
            client_id,
            &self.repository.owner.login,
            &self.repository.name,
            &self.path,
            Some(reference),
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
    #[serde(rename = "ref")]
    pub r#ref: String,
    pub object: GitRefTarget,
}

/// The target object that a Git reference points to.
/// For example, a branch may point to a commit.
#[derive(Deserialize)]
pub struct GitRefTarget {
    /// The type of the object (e.g., "commit", "tag").
    #[serde(rename = "type")]
    pub r#type: String,
    /// The SHA-1 hash of the referenced object.
    pub sha: String,
}

/*******************************************************************************************
   FETCHING FILES
*******************************************************************************************/

/// Request sent to the runtime to fetch a file
#[derive(Serialize, Deserialize)]
pub struct FetchFileRequest {
    pub organization: String,
    pub repository_name: String,
    pub file_path: String,
    pub reference: Option<String>,
    pub media_type: FetchFileCustomMediaType,
}

/// Custom media type passed when fetching a file.
/// For more info, see https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content
#[derive(Serialize, Deserialize)]
pub enum FetchFileCustomMediaType {
    Default,
    Raw,
    Html,
    Object,
}

impl Display for FetchFileCustomMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Raw => write!(f, "raw"),
            Self::Html => write!(f, "html"),
            Self::Object => write!(f, "object"),
        }
    }
}

/*******************************************************************************************
   MISCELLANEA
*******************************************************************************************/

#[derive(Serialize, Deserialize)]
pub struct GithubApiWrapper<T> {
    pub client_id: String,
    pub params: T,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigureSecretParams {
    pub owner: String,
    pub repo: String,
    pub env_name: Option<String>,
    pub secret_name: String,
    pub secret: String,
}

/// A GitHub user.
/// Note - We only deserialize the fields we care about.
#[derive(Debug, Serialize, Deserialize)]
pub struct GitHubUser {
    /// The user's login name. This can change over time.
    pub login: String,
    /// The user's unique ID. This does not change.
    pub id: u64,
}

pub enum ReviewPatAction {
    Approve,
    Deny,
}

#[derive(Debug, Serialize, Deserialize)]
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

/// Parameters sent to the runtime when creating a GH deploy key
#[derive(Serialize, Deserialize)]
pub struct CreateDeployKeyParams {
    pub owner: String,
    pub repo: String,
    pub title: String,
    pub key: String,
    pub read_only: bool,
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

/// Request to create or update a file in a repository.
#[derive(Serialize, Deserialize)]
pub struct CreateOrUpdateFileRequest {
    /// Owner of the repository (e.g., GitHub username or org).
    pub owner: String,
    /// Name of the repository.
    pub repo: String,
    /// Path to create or update the file at
    pub path: String,
    /// The commit message.
    pub message: String,
    /// The new file content,
    pub content: Vec<u8>,
    /// The branch name. Default: the repository’s default branch.
    pub branch: Option<String>,
    /// The blob SHA of the file being replaced. Required if updating a file.
    pub sha: Option<String>,
}

/// Request to comment on a pull request or issue
#[derive(Serialize, Deserialize)]
pub struct CommentOnPullRequestRequest {
    /// Name of the owner of the repository
    pub owner: String,
    /// Name of the repository where the PR or issue is
    pub repository: String,
    /// Number of the pull request or issue
    pub number: String,
    /// Comment to leave
    pub comment: String,
}

/// A team that has access to a repository, along with the permission level it has on the repository.
#[derive(Serialize, Deserialize)]
pub struct GitHubRepoTeam {
    pub slug: String,
    pub permissions: Permission,
}

/*******************************************************************************************
   ORG SECRETS
*******************************************************************************************/

#[derive(Serialize, Deserialize)]
pub struct AddOrRemoveRepoToOrgSecretParams {
    pub org: String,
    pub secret_name: String,
    pub repository: String,
}

#[derive(Serialize, Deserialize)]
pub struct ListOrgSecretsForRepoParams {
    pub org: String,
    pub repository: String,
    pub per_page: Option<u32>,
    pub page: Option<u32>,
}

/*******************************************************************************************
   DEPENDABOT ALERTS
*******************************************************************************************/

/// The state of a Dependabot alert.
/// See https://docs.github.com/en/rest/dependabot/alerts?apiVersion=2026-03-10#list-dependabot-alerts-for-a-repository
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependabotAlertState {
    AutoDismissed,
    Dismissed,
    Fixed,
    Open,
}

impl Display for DependabotAlertState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self {
            Self::AutoDismissed => "auto_dismissed",
            Self::Dismissed => "dismissed",
            Self::Fixed => "fixed",
            Self::Open => "open",
        };
        write!(f, "{state}")
    }
}

/// The severity of a vulnerability, as reported by GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisorySeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// The package affected by a Dependabot alert.
#[derive(Debug, Clone, Deserialize)]
pub struct DependabotAlertPackage {
    /// The package's language or package management ecosystem, e.g. "npm" or "pip".
    pub ecosystem: String,
    /// The unique package name within its ecosystem, e.g. "lodash".
    pub name: String,
}

/// The dependency (i.e. the manifest entry) that a Dependabot alert refers to.
#[derive(Debug, Clone, Deserialize)]
pub struct DependabotAlertDependency {
    pub package: DependabotAlertPackage,
    /// The full path to the manifest file that contains the vulnerable dependency.
    pub manifest_path: Option<String>,
    /// The scope of the vulnerable dependency, either "development" or "runtime".
    pub scope: Option<String>,
    /// The relationship of the vulnerable dependency to the project, e.g. "direct" or "transitive".
    pub relationship: Option<String>,
}

/// An identifier (CVE or GHSA) for a security advisory.
#[derive(Debug, Clone, Deserialize)]
pub struct AdvisoryIdentifier {
    /// The identifier type, either "CVE" or "GHSA".
    #[serde(rename = "type")]
    pub type_: String,
    /// The identifier value, e.g. "CVE-2021-44228".
    pub value: String,
}

/// The security advisory that a Dependabot alert is based on.
/// Note - We only deserialize the fields we care about.
#[derive(Debug, Clone, Deserialize)]
pub struct DependabotSecurityAdvisory {
    /// The GitHub Security Advisory ID, e.g. "GHSA-xxxx-yyyy-zzzz".
    pub ghsa_id: String,
    /// The Common Vulnerabilities and Exposures (CVE) ID, if one exists.
    pub cve_id: Option<String>,
    /// A short, plain-text summary of the advisory.
    pub summary: String,
    /// The severity of the advisory.
    pub severity: AdvisorySeverity,
    /// All identifiers (CVE and GHSA) associated with this advisory.
    #[serde(default)]
    pub identifiers: Vec<AdvisoryIdentifier>,
    /// When the advisory was published.
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub published_at: DateTime<Utc>,
    /// When the advisory was last updated.
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub updated_at: DateTime<Utc>,
}

/// The specific vulnerable version range that a Dependabot alert refers to.
#[derive(Debug, Clone, Deserialize)]
pub struct DependabotSecurityVulnerability {
    pub package: DependabotAlertPackage,
    /// The severity of the vulnerable version range.
    pub severity: AdvisorySeverity,
    /// The range of versions that are vulnerable, e.g. "< 4.17.21".
    pub vulnerable_version_range: String,
    /// The first version that patches the vulnerability, if one exists.
    pub first_patched_version: Option<FirstPatchedVersion>,
}

/// The first version of a package that patches a vulnerability.
#[derive(Debug, Clone, Deserialize)]
pub struct FirstPatchedVersion {
    /// The version identifier of the patched release, e.g. "4.17.21".
    pub identifier: String,
}

/// A Dependabot alert for a repository.
/// Note - We only deserialize the fields we care about. See
/// https://docs.github.com/en/rest/dependabot/alerts?apiVersion=2026-03-10#list-dependabot-alerts-for-a-repository
/// for the full response schema.
#[derive(Debug, Clone, Deserialize)]
pub struct DependabotAlert {
    /// The unique identifier of the alert within the repository.
    pub number: u64,
    /// The current state of the alert.
    pub state: DependabotAlertState,
    /// The dependency that the alert refers to.
    pub dependency: DependabotAlertDependency,
    /// The advisory that the alert is based on.
    pub security_advisory: DependabotSecurityAdvisory,
    /// The vulnerable version range that the alert refers to.
    pub security_vulnerability: DependabotSecurityVulnerability,
    /// The API URL for this alert.
    pub url: String,
    /// The web URL for this alert.
    pub html_url: String,
    /// When the alert was created.
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub created_at: DateTime<Utc>,
    /// When the alert was last updated.
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub updated_at: DateTime<Utc>,
    /// When the alert was dismissed, if it has been.
    #[serde(
        default,
        deserialize_with = "datetime::deserialize_option_rfc3339_timestamp"
    )]
    pub dismissed_at: Option<DateTime<Utc>>,
    /// The reason the alert was dismissed, if it has been.
    pub dismissed_reason: Option<String>,
    /// When the alert was fixed, if it has been.
    #[serde(
        default,
        deserialize_with = "datetime::deserialize_option_rfc3339_timestamp"
    )]
    pub fixed_at: Option<DateTime<Utc>>,
}

/*******************************************************************************************
   CODE SCANNING ALERTS
*******************************************************************************************/

/// The state of a code scanning alert.
/// See https://docs.github.com/en/rest/code-scanning/code-scanning?apiVersion=2026-03-10#list-code-scanning-alerts-for-a-repository
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeScanningAlertState {
    Open,
    Dismissed,
    Fixed,
}

impl Display for CodeScanningAlertState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self {
            Self::Open => "open",
            Self::Dismissed => "dismissed",
            Self::Fixed => "fixed",
        };
        write!(f, "{state}")
    }
}

/// The rule that a code scanning alert was raised by.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeScanningRule {
    /// A unique identifier for the rule used to detect the alert.
    pub id: Option<String>,
    /// The name of the rule used to detect the alert.
    pub name: String,
    /// The severity of the alert, e.g. "error" or "warning".
    pub severity: Option<String>,
    /// The security severity of the alert, e.g. "high" or "critical".
    pub security_severity_level: Option<String>,
    /// A short description of the rule used to detect the alert.
    pub description: String,
}

/// The tool that generated a code scanning alert.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeScanningTool {
    /// The name of the tool used to generate the code scanning analysis, e.g. "CodeQL".
    pub name: String,
    /// The version of the tool used to generate the code scanning analysis.
    pub version: Option<String>,
    /// The GUID of the tool used to generate the code scanning analysis.
    pub guid: Option<String>,
}

/// The location of a code scanning alert instance within a repository.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeScanningAlertLocation {
    /// The file path of the alert, relative to the repository root.
    pub path: String,
    /// The start line of the alert in the file.
    pub start_line: Option<u64>,
    /// The end line of the alert in the file.
    pub end_line: Option<u64>,
    /// The start column of the alert in the file.
    pub start_column: Option<u64>,
    /// The end column of the alert in the file.
    pub end_column: Option<u64>,
}

/// The most recent instance of a code scanning alert.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeScanningAlertInstance {
    /// The Git reference for the analysis that produced the alert.
    pub r#ref: Option<String>,
    /// The state of the alert instance.
    pub state: Option<String>,
    /// The commit SHA of the analysis that produced the alert.
    pub commit_sha: Option<String>,
    /// The location of the alert instance.
    pub location: Option<CodeScanningAlertLocation>,
}

/// A code scanning alert for a repository.
/// Note - We only deserialize the fields we care about. See
/// https://docs.github.com/en/rest/code-scanning/code-scanning?apiVersion=2026-03-10#list-code-scanning-alerts-for-a-repository
/// for the full response schema.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeScanningAlert {
    /// The unique identifier of the alert within the repository.
    pub number: u64,
    /// The current state of the alert.
    pub state: CodeScanningAlertState,
    /// The rule that the alert was raised by.
    pub rule: CodeScanningRule,
    /// The tool that generated the alert.
    pub tool: CodeScanningTool,
    /// The most recent instance of the alert.
    pub most_recent_instance: Option<CodeScanningAlertInstance>,
    /// The API URL for this alert.
    pub url: String,
    /// The web URL for this alert.
    pub html_url: String,
    /// When the alert was created.
    #[serde(deserialize_with = "datetime::deserialize_rfc3339_timestamp")]
    pub created_at: DateTime<Utc>,
    /// When the alert was last updated.
    #[serde(
        default,
        deserialize_with = "datetime::deserialize_option_rfc3339_timestamp"
    )]
    pub updated_at: Option<DateTime<Utc>>,
    /// When the alert was dismissed, if it has been.
    #[serde(
        default,
        deserialize_with = "datetime::deserialize_option_rfc3339_timestamp"
    )]
    pub dismissed_at: Option<DateTime<Utc>>,
    /// The reason the alert was dismissed, if it has been.
    pub dismissed_reason: Option<String>,
    /// When the alert was fixed, if it has been.
    #[serde(
        default,
        deserialize_with = "datetime::deserialize_option_rfc3339_timestamp"
    )]
    pub fixed_at: Option<DateTime<Utc>>,
}
