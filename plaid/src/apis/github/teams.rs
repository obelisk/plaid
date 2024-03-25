use std::collections::HashMap;

use super::Github;
use crate::apis::{ApiError, github::GitHubError};

impl Github {
    pub async fn remove_user_from_team(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = request.get("org").ok_or(ApiError::BadRequest)?;
        let team_slug = request.get("team_slug").ok_or(ApiError::BadRequest)?;
        let user = request.get("user").ok_or(ApiError::BadRequest)?;

        println!("Removing user [{user}] from [{team_slug}] in [{org}]");
        let address = format!("https://api.github.com/orgs/{org}/teams/{team_slug}/memberships/{user}");
        let request = self.client
            .delete(address)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token));

        match request.send().await {
            Ok(r) => if r.status() == 204 {
                Ok(0)
            } else {
                Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(r.status().as_u16())))
            },
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    pub async fn add_user_to_team(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = request.get("org").ok_or(ApiError::BadRequest)?;
        let team_slug = request.get("team_slug").ok_or(ApiError::BadRequest)?;
        let user = request.get("user").ok_or(ApiError::BadRequest)?;
        let role = request.get("role").ok_or(ApiError::BadRequest)?;

        println!("Adding user [{user}] to [{team_slug}] in [{org}] as [{role}]");
        let address = format!("https://api.github.com/orgs/{org}/teams/{team_slug}/memberships/{user}");
        let request = self.client
            .put(address)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token))
            .body(format!("{{\"role\": \"{role}\"}}"));

        match request.send().await {
            Ok(r) => if r.status() == 200 {
                Ok(0)
            } else {
                Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(r.status().as_u16())))
            },
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}