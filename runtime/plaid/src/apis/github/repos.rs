use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use plaid_stl::github::{
    AddUserToRepoParams, CheckCodeownersParams, CodeownersErrorsResponse, CodeownersStatus,
    CommentOnPullRequestRequest, CreateOrUpdateFileRequest, FetchCommitParams,
    FetchFileCustomMediaType, FetchFileRequest, GetBranchProtectionRulesParams,
    GetBranchProtectionRulesetParams, GetCustomPropertiesValuesParams, GetRepoCollaboratorsParams,
    GetRepoIdFromRepoNameParams, GetRepoNameFromRepoIdParams, GetRepoSbomParams,
    GetWeeklyCommitCountParams, GithubApiWrapper, GithubRepository, ListFilesParams,
    RemoveUserFromRepoParams, RequireSignedCommitsParams, UpdateBranchProtectionRuleParams,
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
        let request: GithubApiWrapper<RemoveUserFromRepoParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(&request.params.user)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        let address = format!("/repos/{repo}/collaborators/{user}");
        info!("Removing user [{user}] from [{repo}] on behalf of {module}");

        match self
            .make_generic_delete_request::<&str>(&request.client_id, address, None, module)
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
        let request: GithubApiWrapper<AddUserToRepoParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(&request.params.user)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let permission = request.params.permission.as_deref().unwrap_or("pull");

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
            .make_generic_put_request(&request.client_id, address, Some(&permission), module)
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
        let request: GithubApiWrapper<FetchCommitParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let user = self.validate_username(&request.params.user)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let commit = self.validate_commit_hash(&request.params.commit)?;

        info!("Fetching commit [{commit}] from [{repo}] by [{user}] on behalf of {module}");
        let address = format!("/repos/{user}/{repo}/commits/{commit}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Returns a list of all Files in a Pull Request.
    /// See https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28#list-pull-requests-files for more detail
    pub async fn list_files(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<ListFilesParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.params.owner)?;
        let repository_name = self.validate_repository_name(&request.params.repo)?;
        let pull_request = self.validate_pint(&request.params.pull_request)?;
        let page = self.validate_pint(&request.params.page)?;

        info!("Fetching files for Pull Request Nr {pull_request} from [{organization}/{repository_name}] on behalf of {module}");
        let address = format!(
            "/repos/{organization}/{repository_name}/pulls/{pull_request}/files?page={page}"
        );

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Returns the contents of a file at a specific URI.
    /// See https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content for more detail
    pub async fn fetch_file_with_custom_media_type(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<FetchFileRequest> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.params.organization)?;
        let repository_name = self.validate_repository_name(&request.params.repository_name)?;
        let file_path = &request.params.file_path;

        // If this call return Ok(_), it means the provided file path contains ".." which we do
        // not want to allow
        if self
            .validate_contains_parent_directory_component(file_path)
            .is_ok()
        {
            return Err(ApiError::BadRequest);
        }

        // If a reference is given (and passes validation), we will log about it and append it to the request URL as a query parameter.
        // If it is not given, we will omit that part of the log and append nothing to the URL.
        let (ref_log, ref_q_param) = match request.params.reference {
            Some(r) => {
                // According to https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content,
                // `reference` can be a commit SHA, a branch name, or a tag name. We validate that it is either a 40-char SHA-1,
                // or a branch/tag name that matches our `branch_name` validator (see validators.rs).
                // This means that we try validating both ways: if both fail, we return an error. If either succeeds, we continue.
                if self.validate_commit_hash(&r).is_err() && self.validate_branch_name(&r).is_err()
                {
                    return Err(ApiError::BadRequest);
                }
                //               log                       query param
                (format!(" and reference [{r}]"), format!("?ref={r}"))
            }
            None => (String::new(), String::new()),
        };

        let custom_media_type = request.params.media_type;

        info!("Fetching contents of file in repository [{organization}/{repository_name}] at [{file_path}]{ref_log} with encoding [{custom_media_type}]");
        let address =
            format!("/repos/{organization}/{repository_name}/contents/{file_path}{ref_q_param}");

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
            .make_get_request_with_headers(&request.client_id, address, Some(headers), module)
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
        let request: GithubApiWrapper<GetBranchProtectionRulesParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;

        let repo = self.validate_repository_name(&request.params.repo)?;
        let branch = self.validate_branch_name(&request.params.branch)?;

        info!("Fetching branch protection rules for branch [{branch}] in repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/branches/{branch}/protection");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Fetches branch protection rules (as in rulesets).
    /// See https://docs.github.com/en/rest/repos/rules?apiVersion=2022-11-28#get-rules-for-a-branch for more detail
    pub async fn get_branch_protection_ruleset(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetBranchProtectionRulesetParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let branch = self.validate_branch_name(&request.params.branch)?;

        info!("Fetching branch protection rules (as in rulesets) for branch [{branch}] in repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/rules/branches/{branch}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Get collaborators on a repository, with support for paginated responses.
    /// Optionally supports specifying how many results each page should contain (default=30, max=100) and which page is requested (default=1).
    /// Repeatedly calling this function with different page numbers allows one to get all collaborators on a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#list-repository-collaborators for more detail
    pub async fn get_repository_collaborators(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetRepoCollaboratorsParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let per_page: u8 = request.params.per_page.unwrap_or(30);
        let page: u16 = request.params.page.unwrap_or(1);
        let affiliation = request.params.affiliation.as_deref().unwrap_or("all");
        // See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2026-03-10#list-repository-collaborators
        // for more details on the possible values for affiliation
        if !["outside", "direct", "all"].contains(&affiliation) {
            return Err(ApiError::BadRequest);
        }

        if per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        info!("Fetching collaborators for [{repo}] on behalf of {module}");
        let address =
            format!("/repos/{owner}/{repo}/collaborators?per_page={per_page}&affiliation={affiliation}&page={page}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Update (or create) a branch protection rule.
    /// See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#update-branch-protection for more detail
    pub async fn update_branch_protection_rule(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<UpdateBranchProtectionRuleParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let branch = self.validate_branch_name(&request.params.branch)?;
        let body = &request.params.body;
        // We deserialize to a Value. This has two effects: (1) it checks that we received valid JSON,
        // and (2) it prepares the body that will be sent later with the request.
        let body =
            serde_json::from_str::<serde_json::Value>(&body).map_err(|_| ApiError::BadRequest)?;

        info!("Updating branch protection rules for branch [{branch}] in repo [{owner}/{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/branches/{branch}/protection");

        match self
            .make_generic_put_request(&request.client_id, address, Some(&body), module)
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
        let request: GithubApiWrapper<RequireSignedCommitsParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let branch = self.validate_branch_name(&request.params.branch)?;
        let activated = request.params.activated;

        let address =
            format!("/repos/{owner}/{repo}/branches/{branch}/protection/required_signatures");

        // Turned signed commits on or off dependending on the value of `activated`
        if activated {
            info!("Enabling signed commits requirement for branch [{branch}] in repo [{owner}/{repo}] on behalf of {module}");

            match self
                .make_generic_post_request(&request.client_id, address, None::<String>, module)
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
                .make_generic_delete_request(&request.client_id, address, None::<&String>, module)
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
        let request: GithubApiWrapper<GetWeeklyCommitCountParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        info!("Fetching weekly commit count for repo [{owner}/{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/stats/participation");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Comment on a Pull Request
    /// See https://docs.github.com/en/rest/issues/comments?apiVersion=2022-11-28#create-an-issue-comment for more detail
    pub async fn comment_on_pull_request(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<CommentOnPullRequestRequest> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let username = self.validate_username(&request.params.owner)?;
        let repository_name = self.validate_repository_name(&request.params.repository)?;
        let pull_request = self.validate_pint(&request.params.number)?;
        let comment = &request.params.comment;

        info!("Commenting on Pull Request [{pull_request}] in repo [{repository_name}] on behalf of {module}");
        let address = format!("/repos/{username}/{repository_name}/issues/{pull_request}/comments");

        #[derive(Serialize)]
        struct Body<'a> {
            body: &'a str,
        }

        match self
            .make_generic_post_request(&request.client_id, address, Body { body: comment }, module)
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
        let request: GithubApiWrapper<GetCustomPropertiesValuesParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        info!("Getting custom properties of repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/properties/values");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Check a repo's CODEOWNERS file and return whether it is OK, missing or invalid (i.e., contains errors).
    /// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors for more detail
    pub async fn check_codeowners_file(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<CheckCodeownersParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        info!("Checking CODEOWNERS for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}/codeowners/errors");

        match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
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
        let request: GithubApiWrapper<CreateOrUpdateFileRequest> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_org(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let path = self.validate_path(&request.params.path)?;
        let file_hash = sha256_hex(&request.params.content);

        let address = format!("/repos/{owner}/{repo}/contents/{path}");

        let mut body = json!({
            "message": request.params.message,
            "content": base64::encode(&request.params.content),
        });
        if let Some(branch) = request.params.branch {
            body["branch"] = json!(branch);
        }

        if let Some(sha) = request.params.sha {
            // Validate: even if this is not a commit hash but a blob hash, the format is still the same
            self.validate_commit_hash(&sha)?;
            body["sha"] = json!(sha);
        }

        info!("Creating file with hash [{file_hash}] at [{path}] in repository [{owner}/{repo}] on behalf of [{module}]");

        match self
            .make_generic_put_request(&request.client_id, address, Some(&body), module)
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
        let request: GithubApiWrapper<GetRepoSbomParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        info!("Fetching SBOM for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}/dependency-graph/sbom");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Get a repo ID from its name
    /// See https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#get-a-repository
    pub async fn get_repo_id_from_repo_name(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetRepoIdFromRepoNameParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        self.get_repo_id_from_repo_name_internal(&request.client_id, &owner, &repo, module)
            .await
    }

    /// Internal function to get a repo ID from its name, so that it can be called from different places in the runtime
    pub async fn get_repo_id_from_repo_name_internal(
        &self,
        client_id: &str,
        owner: &str,
        repo: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        info!("Getting repo ID for repo [{owner}/{repo}] on behalf of [{module}]");
        let address = format!("/repos/{owner}/{repo}");

        match self
            .make_generic_get_request(client_id, address, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    let repo_info: GithubRepository = serde_json::from_str(&body)
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
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetRepoNameFromRepoIdParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let repo_id = self.validate_repo_id(&request.params.repo_id)?;

        info!("Getting repo name for repo ID [{repo_id}] on behalf of [{module}]");
        let address = format!("/repositories/{repo_id}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
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
