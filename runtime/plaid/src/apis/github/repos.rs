use std::{collections::HashMap, sync::Arc};

use http::{HeaderMap, HeaderValue};
use octocrab::models::events::Repository;
use plaid_stl::github::{
    CheckCodeownersParams, CodeownersErrorsResponse, CodeownersStatus, CommentOnPullRequestRequest,
    CreateFileRequest, FetchFileCustomMediaType, FetchFileRequest, GithubRepository,
};
use serde::Serialize;
use serde_json::json;

use crate::{
    apis::{github::GitHubError, ApiError},
    cryptography::hash::sha256_hex,
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// Removes a collaborator from a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#remove-a-repository-collaborator for more detail
    pub async fn remove_user_from_repo(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        let address = format!("/repos/{repo}/collaborators/{user}");
        info!("Removing user [{user}] from [{repo}] on behalf of {module}");

        match self
            .make_generic_delete_request::<&str>(address, None, module)
            .await
        {
            Ok((status, _)) => {
                if status == 204 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Adds a collaborator to a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#add-a-repository-collaborator for more detail
    pub async fn add_user_to_repo(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let permission = request.get("permission").unwrap_or(&"pull");

        info!(
            "Adding user [{user}] to [{repo}] with permission [{permission}] on behalf of {module}"
        );
        let address = format!("/repos/{repo}/collaborators/{user}");

        #[derive(Serialize)]
        struct Permission {
            permission: String,
        }

        let permission = Permission {
            permission: permission.to_string(),
        };

        match self
            .make_generic_put_request(address, Some(&permission), module)
            .await
        {
            Ok((status, _)) => {
                // https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#add-a-repository-collaborator--status-codes
                // The response is 204 when the collaborator is added, and 201 when a new invitation is created
                if status == 204 || status == 201 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Returns the contents of a single commit reference.
    /// See https://docs.github.com/en/rest/commits/commits?apiVersion=2022-11-28#get-a-commit for more detail
    pub async fn fetch_commit(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let commit =
            self.validate_commit_hash(request.get("commit").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching commit [{commit}] from [{repo}] by [{user}] on behalf of {module}");
        let address = format!("/repos/{user}/{repo}/commits/{commit}");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Returns a list of all Files in a Pull Request.
    /// See https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28#list-pull-requests-files for more detail
    pub async fn list_files(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization =
            self.validate_org(request.get("organization").ok_or(ApiError::BadRequest)?)?;
        let repository_name = self.validate_repository_name(
            request.get("repository_name").ok_or(ApiError::BadRequest)?,
        )?;
        let pull_request =
            self.validate_pint(request.get("pull_request").ok_or(ApiError::BadRequest)?)?;
        let page = self.validate_pint(request.get("page").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching files for Pull Request Nr {pull_request} from [{organization}/{repository_name}] on behalf of {module}");
        let address = format!(
            "/repos/{organization}/{repository_name}/pulls/{pull_request}/files?page={page}"
        );

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Returns the contents of a file at a specific URI.
    /// See https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content for more detail
    pub async fn fetch_file_with_custom_media_type(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: FetchFileRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.organization)?;
        let repository_name = self.validate_repository_name(&request.repository_name)?;
        let file_path = &request.file_path;

        // If this call return Ok(_), it means the provided file path contains ".." which we do
        // not want to allow
        if self
            .validate_contains_parent_directory_component(file_path)
            .is_ok()
        {
            return Err(ApiError::BadRequest);
        }

        let reference = request.reference;

        // Reference can be commit hash OR a branch name.
        // To validate that the provided ref is valid, we must check that it is either a
        // commit hash or branch name using the provided validator functions
        if self.validate_commit_hash(&reference).is_err()
            && self.validate_branch_name(&reference).is_err()
        {
            return Err(ApiError::BadRequest);
        }

        let custom_media_type = request.media_type;

        info!("Fetching contents of file in repository [{organization}/{repository_name}] at {file_path} and reference {reference} with encoding [{custom_media_type}]");
        let address =
            format!("/repos/{organization}/{repository_name}/contents/{file_path}?ref={reference}");

        // Set the Accept header according to the media type we have
        let header_value = match custom_media_type {
            FetchFileCustomMediaType::Default => "application/vnd.github+json".to_string(),
            cmt => format!("application/vnd.github.{cmt}+json"),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "Accept",
            HeaderValue::from_str(&header_value).map_err(|_| ApiError::ImpossibleError)?,
        );

        match self
            .make_get_request_with_headers(address, Some(headers), module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Fetches branch protection rules.
    /// See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#get-branch-protection for more detail
    pub async fn get_branch_protection_rules(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching branch protection rules for branch [{branch}] in repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/branches/{branch}/protection");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Fetches branch protection rules (as in rulesets).
    /// See https://docs.github.com/en/rest/repos/rules?apiVersion=2022-11-28#get-rules-for-a-branch for more detail
    pub async fn get_branch_protection_ruleset(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching branch protection rules (as in rulesets) for branch [{branch}] in repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/rules/branches/{branch}");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Get collaborators on a repository, with support for paginated responses.
    /// Optionally supports specifying how many results each page should contain (default=30, max=100) and which page is requested (default=1).
    /// Repeatedly calling this function with different page numbers allows one to get all collaborators on a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#list-repository-collaborators for more detail
    pub async fn get_repository_collaborators(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let per_page: u8 = request
            .get("per_page")
            .unwrap_or(&"30")
            .parse::<u8>()
            .map_err(|_| ApiError::BadRequest)?;
        let page: u16 = request
            .get("page")
            .unwrap_or(&"1")
            .parse::<u16>()
            .map_err(|_| ApiError::BadRequest)?;

        if per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        info!("Fetching collaborators for [{repo}] on behalf of {module}");
        let address =
            format!("/repos/{owner}/{repo}/collaborators?per_page={per_page}&page={page}");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Update (or create) a branch protection rule.
    /// See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#update-branch-protection for more detail
    pub async fn update_branch_protection_rule(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;
        let body = request.get("body").ok_or(ApiError::BadRequest)?;
        // We deserialize to a Value. This has two effects: (1) it checks that we received valid JSON,
        // and (2) it prepares the body that will be sent later with the request.
        let body =
            serde_json::from_str::<serde_json::Value>(&body).map_err(|_| ApiError::BadRequest)?;

        info!("Updating branch protection rules for branch [{branch}] in repo [{owner}/{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/branches/{branch}/protection");

        match self
            .make_generic_put_request(address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Require signed commits on a given branch of a given repo.
    /// See https://docs.github.com/en/enterprise-cloud@latest/rest/branches/branch-protection?apiVersion=2022-11-28#create-commit-signature-protection for more detail
    pub async fn require_signed_commits(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;
        let activated = match request
            .get("activated")
            .ok_or(ApiError::BadRequest)?
            .to_string()
            .as_str()
        {
            "true" => true,
            "false" => false,
            _ => return Err(ApiError::BadRequest),
        };

        let address =
            format!("/repos/{owner}/{repo}/branches/{branch}/protection/required_signatures");

        // Turned signed commits on or off dependending on the value of `activated`
        if activated {
            info!("Enabling signed commits requirement for branch [{branch}] in repo [{owner}/{repo}] on behalf of {module}");

            match self
                .make_generic_post_request(address, None::<String>, module)
                .await
            {
                Ok((status, _)) => {
                    if status == 200 {
                        Ok(0)
                    } else {
                        Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                            status,
                        )))
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            info!("Disabling signed commits requirement for branch [{branch}] in repo [{owner}/{repo}] on behalf of {module}");

            match self
                .make_generic_delete_request(address, None::<&String>, module)
                .await
            {
                Ok((status, _)) => {
                    if status == 204 {
                        Ok(0)
                    } else {
                        Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                            status,
                        )))
                    }
                }
                Err(e) => Err(e),
            }
        }
    }

    /// Get the weekly commit count on a given repo.
    /// See https://docs.github.com/en/rest/metrics/statistics?apiVersion=2022-11-28#get-the-weekly-commit-count for more detail
    pub async fn get_weekly_commit_count(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching weekly commit count for repo [{owner}/{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/stats/participation");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Comment on a Pull Request
    /// See https://docs.github.com/en/rest/issues/comments?apiVersion=2022-11-28#create-an-issue-comment for more detail
    pub async fn comment_on_pull_request(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: CommentOnPullRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let username = self.validate_username(&request.owner)?;
        let repository_name = self.validate_repository_name(&request.repository)?;
        let pull_request = self.validate_pint(&request.number)?;
        let comment = &request.comment;

        info!("Commenting on Pull Request [{pull_request}] in repo [{repository_name}] on behalf of {module}");
        let address = format!("/repos/{username}/{repository_name}/issues/{pull_request}/comments");

        #[derive(Serialize)]
        struct Body<'a> {
            body: &'a str,
        }

        match self
            .make_generic_post_request(address, Body { body: comment }, module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Get all custom property values for a repository
    /// See https://docs.github.com/en/rest/repos/custom-properties?apiVersion=2022-11-28#get-all-custom-property-values-for-a-repository
    pub async fn get_custom_properties_values(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Getting custom properties of repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/properties/values");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Check a repo's CODEOWNERS file and return whether it is OK, missing or invalid (i.e., contains errors).
    /// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors for more detail
    pub async fn check_codeowners_file(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: CheckCodeownersParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;

        info!("Checking CODEOWNERS for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}/codeowners/errors");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    // Deserialize the body and see if we had errors
                    match serde_json::from_str::<CodeownersErrorsResponse>(&body) {
                        Err(_) => {
                            debug!("Checking CODEOWNERS for repo [{owner}/{repo}] resulted in an error");
                            Err(ApiError::GitHubError(GitHubError::BadResponse))
                        }
                        Ok(response) => {
                            if response.errors.is_empty() {
                                debug!("Checking CODEOWNERS for repo [{owner}/{repo}] returned 200 and no errors");
                                serde_json::to_string(&CodeownersStatus::Ok)
                                    .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
                            } else {
                                // Errors have been detected
                                debug!("Checking CODEOWNERS for repo [{owner}/{repo}] returned 200 but detected errors. The body was [{body}] and the response was [{response:?}]");
                                serde_json::to_string(&CodeownersStatus::Invalid(response.errors))
                                    .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
                            }
                        }
                    }
                } else if status == 404 {
                    debug!("Checking CODEOWNERS for repo [{owner}/{repo}] returned 404");
                    serde_json::to_string(&CodeownersStatus::Missing)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
                } else {
                    debug!("Checking CODEOWNERS for repo [{owner}/{repo}] returned {status}");
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Creates a new file in a repository.
    pub async fn create_file(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: CreateFileRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_org(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;
        let path = self.validate_path(&request.path)?;
        let file_hash = sha256_hex(&request.content);

        info!("Creating file with hash [{file_hash}] at [{path}] in repository [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}/contents/{path}");

        let mut body = json!({
            "message": request.message,
            "content": base64::encode(&request.content),
        });
        if let Some(branch) = request.branch {
            body["branch"] = json!(branch);
        }

        match self
            .make_generic_put_request(address, Some(&body), module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 200 || status == 201 {
                    Ok(file_hash)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Fetches the software bill of materials (SBOM) for a repository in SPDX JSON format.
    /// See https://docs.github.com/en/rest/dependency-graph/sboms?apiVersion=2022-11-28#export-a-software-bill-of-materials-sbom-for-a-repository for more detail
    pub async fn get_repo_sbom(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching SBOM for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}/dependency-graph/sbom");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Get a repo ID from its name
    /// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#get-a-repository
    pub async fn get_repo_id_from_repo_name(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Getting repo ID for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    let repo_info: Repository = serde_json::from_str(&body)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;
                    Ok(repo_info.id.to_string())
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// Get a repo's name from their repo ID
    /// This is not explicitly documented in the API specs but it works, even for private repos.
    /// Related to https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#get-a-repository
    pub async fn get_repo_name_from_repo_id(
        &self,
        repo_id: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let repo_id = self.validate_repo_id(repo_id)?;

        info!("Getting repo name for repo ID [{repo_id}] on behalf of [{module}]");
        let address = format!("/repositories/{repo_id}");

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    let repo_info: GithubRepository = serde_json::from_str(&body)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;
                    Ok(repo_info.full_name)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }
}
