use std::collections::HashMap;

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

impl Github {
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

        let permission = format!("{{\"permission\": \"{permission}\"}}");

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
}
