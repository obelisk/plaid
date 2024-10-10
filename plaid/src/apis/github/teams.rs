use std::collections::HashMap;

use serde::Serialize;

use super::Github;
use crate::apis::{github::GitHubError, ApiError};

impl Github {
    pub async fn remove_user_from_team(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let team_slug =
            self.validate_team_slug(request.get("team_slug").ok_or(ApiError::BadRequest)?)?;
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;

        info!("Removing user [{user}] from [{team_slug}] in [{org}] on behalf of {module}");
        let address = format!("/orgs/{org}/teams/{team_slug}/memberships/{user}");

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

    pub async fn add_user_to_team(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let team_slug =
            self.validate_team_slug(request.get("team_slug").ok_or(ApiError::BadRequest)?)?;
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;

        let role = request.get("role").ok_or(ApiError::BadRequest)?;

        info!("Adding user [{user}] to [{team_slug}] in [{org}] as [{role}] on behalf of {module}");
        #[derive(Serialize)]
        struct Role {
            role: String,
        }

        let role = Role {
            role: role.to_string(),
        };

        let address = format!("/orgs/{org}/teams/{team_slug}/memberships/{user}");

        match self
            .make_generic_put_request(address, Some(&role), module)
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
