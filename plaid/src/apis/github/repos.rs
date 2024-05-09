use std::collections::HashMap;

use serde::Serialize;

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

impl Github {
    /// Removes a collaborator from a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#remove-a-repository-collaborator for more detail
    pub async fn remove_user_from_repo(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        let address = format!("/repos/{repo}/collaborators/{user}",);
        info!("Removing user [{}] from [{}]", user, repo);

        match self
            .make_generic_delete_request(address, None, module)
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
    pub async fn add_user_to_repo(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let permission = request.get("permission").unwrap_or(&"pull");

        info!("Adding user [{user}] to [{repo}] with permission [{permission}]");
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
    pub async fn fetch_commit(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let commit =
            self.validate_commit_hash(request.get("commit").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching commit [{commit}] from [{repo}] by [{user}]");
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

    /// Fetches branch protection rules.
    /// See https://docs.github.com/en/rest/branches/branch-protection?apiVersion=2022-11-28#get-branch-protection for more detail
    pub async fn get_branch_protection_rules(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching branch protection rules for branch [{branch}] in repo [{repo}]");
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

    /// Get all collaborators on a repository.
    /// See https://docs.github.com/en/rest/collaborators/collaborators?apiVersion=2022-11-28#list-repository-collaborators for more detail
    pub async fn get_repository_collaborators(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching collaborators for [{repo}]");
        let address = format!("/repos/{owner}/{repo}/collaborators");

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
        module: &str,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;
        let body = request.get("body").ok_or(ApiError::BadRequest)?;

        info!("Updating branch protection rules for branch [{branch}] in repo [{repo}]");
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
}
