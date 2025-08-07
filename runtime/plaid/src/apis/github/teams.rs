use std::{collections::HashMap, sync::Arc};

use serde::Serialize;

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use std::str::FromStr;

enum Permission {
    Admin,
    Push,
    Maintain,
    Triage,
    Pull,
}

impl FromStr for Permission {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Permission::Admin),
            "push" => Ok(Permission::Push),
            "maintain" => Ok(Permission::Maintain),
            "triage" => Ok(Permission::Triage),
            "pull" => Ok(Permission::Pull),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Permission::Admin => "admin",
            Permission::Push => "push",
            Permission::Maintain => "maintain",
            Permission::Triage => "triage",
            Permission::Pull => "pull",
        };
        write!(f, "{}", s)
    }
}

#[derive(Serialize)]
struct PermissionPayload {
    permission: String,
}

impl From<&Permission> for PermissionPayload {
    fn from(p: &Permission) -> Self {
        Self {
            permission: p.to_string(),
        }
    }
}

impl Github {
    /// Remove a user from a GitHub team.
    pub async fn remove_user_from_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
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

    /// Add a user to a GitHub team.
    pub async fn add_user_to_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
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

    /// Add a repo to a GH team, with a given permission.
    pub async fn add_repo_to_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters
        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let team_slug =
            self.validate_team_slug(request.get("team_slug").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let permission = request
            .get("permission")
            .and_then(|p| Permission::from_str(*p).ok())
            .ok_or(ApiError::BadRequest)?;

        info!("Adding team [{team_slug}] to repo [{repo}] with permission [{permission}] on behalf of [{module}]");
        let address = format!("/orgs/{org}/teams/{team_slug}/repos/{org}/{repo}");

        let permission_payload = PermissionPayload::from(&permission);

        match self
            .make_generic_put_request(address, Some(&permission_payload), module)
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

    /// Remove a repo from a GH team.
    pub async fn remove_repo_from_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters
        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let team_slug =
            self.validate_team_slug(request.get("team_slug").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;

        info!("Removing team [{team_slug}] from repo [{repo}] on behalf of [{module}]");
        let address = format!("/orgs/{org}/teams/{team_slug}/repos/{org}/{repo}");

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
}
