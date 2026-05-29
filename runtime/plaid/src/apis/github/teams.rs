use std::sync::Arc;

use plaid_stl::github::{
    AddRepoToTeamParams, AddUserToTeamParams, GetRepoTeamsParams, GithubApiWrapper,
    RemoveRepoFromTeamParams, RemoveUserFromTeamParams,
};
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
        let request: GithubApiWrapper<RemoveUserFromTeamParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(&request.params.org)?;
        let team_slug = self.validate_team_slug(&request.params.team_slug)?;
        let user = self.validate_username(&request.params.user)?;

        info!("Removing user [{user}] from [{team_slug}] in [{org}] on behalf of {module}");
        let address = format!("/orgs/{org}/teams/{team_slug}/memberships/{user}");

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

    /// Add a user to a GitHub team.
    pub async fn add_user_to_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<AddUserToTeamParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(&request.params.org)?;
        let team_slug = self.validate_team_slug(&request.params.team_slug)?;
        let user = self.validate_username(&request.params.user)?;

        let role = &request.params.role;

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
            .make_generic_put_request(&request.client_id, address, Some(&role), module)
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
        let request: GithubApiWrapper<AddRepoToTeamParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters
        let org = self.validate_org(&request.params.org)?;
        let team_slug = self.validate_team_slug(&request.params.team_slug)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let permission =
            Permission::from_str(&request.params.permission).map_err(|_| ApiError::BadRequest)?;

        info!("Adding team [{team_slug}] to repo [{repo}] with permission [{permission}] on behalf of [{module}]");
        let address = format!("/orgs/{org}/teams/{team_slug}/repos/{org}/{repo}");

        let permission_payload = PermissionPayload::from(&permission);

        match self
            .make_generic_put_request(
                &request.client_id,
                address,
                Some(&permission_payload),
                module,
            )
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
        let request: GithubApiWrapper<RemoveRepoFromTeamParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters
        let org = self.validate_org(&request.params.org)?;
        let team_slug = self.validate_team_slug(&request.params.team_slug)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        info!("Removing team [{team_slug}] from repo [{repo}] on behalf of [{module}]");
        let address = format!("/orgs/{org}/teams/{team_slug}/repos/{org}/{repo}");

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

    /// Get the teams that have access to a repository.
    pub async fn get_repo_teams(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetRepoTeamsParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters
        let org = self.validate_org(&request.params.org)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        let per_page: u8 = request.params.per_page.unwrap_or(30);
        let page: u16 = request.params.page.unwrap_or(1);

        info!("Getting teams with access to repo [{repo}] on behalf of [{module}]");
        let address = format!("/repos/{org}/{repo}/teams?per_page={per_page}&page={page}");

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
}
