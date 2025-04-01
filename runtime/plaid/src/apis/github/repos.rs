use std::{collections::HashMap, sync::Arc};

use serde::Serialize;

use crate::{
    apis::{github::GitHubError, ApiError},
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
        let address = format!("/repos/{organization}/{repository_name}/pulls/{pull_request}/files?page={page}");

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
    pub async fn fetch_file(
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
        let file_path = request.get("file_path").ok_or(ApiError::BadRequest)?;

        // If this call return Ok(_), it means the provided file path contains ".." which we do
        // not want to allow
        if self
            .validate_contains_parent_directory_component(file_path)
            .is_ok()
        {
            return Err(ApiError::BadRequest);
        }

        let reference = request.get("reference").ok_or(ApiError::BadRequest)?;

        // Reference can be commit hash OR a branch name.
        // To validate that the provided ref is valid, we must check that it is either a
        // commit hash or branch name using the provided validator functions
        if self.validate_commit_hash(&reference).is_err()
            && self.validate_branch_name(&reference).is_err()
        {
            return Err(ApiError::BadRequest);
        }

        info!("Fetching contents of file in repository [{organization}/{repository_name}] at {file_path} and reference {reference}");
        let address =
            format!("/repos/{organization}/{repository_name}/contents/{file_path}?ref={reference}");

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
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;
        let body = request.get("body").ok_or(ApiError::BadRequest)?;

        info!("Updating branch protection rules for branch [{branch}] in repo [{repo}] on behalf of {module}");
        let address = format!("/repos/{owner}/{repo}/branches/{branch}/protection");

        match self
            .make_generic_put_request(address, Some(body), module)
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

    /// Comment on a Pull Request
    /// See https://docs.github.com/en/rest/issues/comments?apiVersion=2022-11-28#create-an-issue-comment for more detail
    pub async fn comment_on_pull_request(
        &self,
        params: &str,
        module: &str,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let username = self.validate_username(request.get("usename").ok_or(ApiError::BadRequest)?)?;
        let repository_name =
            self.validate_repository_name(request.get("repository_name").ok_or(ApiError::BadRequest)?)?;
        let pull_request = self.validate_pint(request.get("pull_request").ok_or(ApiError::BadRequest)?)?;
        let comment = request.get("comment").ok_or(ApiError::BadRequest)?;

        info!("Commenting on Pull Request [{pull_request}] in repo [{repository_name}] on behalf of {module}");
        let address = format!("/repos/{username}/{repository_name}/issues/{pull_request}/comments");

        #[derive(Serialize)]
        struct Body<'a> {
            body: &'a str
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
}
