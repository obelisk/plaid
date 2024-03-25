use std::collections::HashMap;

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

impl Github {
    pub async fn remove_user_from_repo(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = request.get("user").ok_or(ApiError::BadRequest)?;
        let repo = request.get("repo").ok_or(ApiError::BadRequest)?;

        println!("Removing user [{}] from [{}]", user, repo);
        let address = format!(
            "https://api.github.com/repos/{}/collaborators/{}",
            repo, user
        );

        let request = self
            .client
            .delete(address)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token));

        match request.send().await {
            Ok(r) => {
                if r.status() == 204 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        r.status().as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    pub async fn add_user_to_repo(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let user = request.get("user").ok_or(ApiError::BadRequest)?;
        let repo = request.get("repo").ok_or(ApiError::BadRequest)?;
        let permission = request.get("permission").unwrap_or(&"pull");

        println!("Adding user [{user}] to [{repo}] with permission [{permission}]");
        let address = format!("https://api.github.com/repos/{repo}/collaborators/{user}");

        let request = self
            .client
            .put(address)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token))
            .body(format!("{{\"permission\": \"{permission}\"}}"));

        match request.send().await {
            Ok(r) => {
                if r.status() == 204 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        r.status().as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    pub async fn fetch_commit(&self, params: &str, _: &str) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let user = request.get("user").ok_or(ApiError::BadRequest)?;
        let repo = request.get("repo").ok_or(ApiError::BadRequest)?;
        let commit = request.get("commit").ok_or(ApiError::BadRequest)?;

        let address = format!(" https://api.github.com/repos/{user}/{repo}/commits/{commit}");

        let request = self
            .client
            .get(address)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("token {}", self.config.token));

        match request.send().await {
            Ok(r) => {
                if r.status() == 200 {
                    Ok(r.text().await.unwrap_or_default())
                } else {
                    let status = r.status();
                    println!("{}", r.text().await.unwrap());
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status.as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}
