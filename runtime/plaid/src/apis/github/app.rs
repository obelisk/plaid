use std::{collections::HashMap, sync::Arc};

use plaid_stl::github::{
    InstallationAccessToken, InstallationAccessTokenPermissionKey,
    InstallationAccessTokenPermissionValue, InstallationAccessTokenRequest,
    InstallationAccessTokenScope,
};
use serde::{Deserialize, Serialize};

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::{build_github_app_client, Github};

impl Github {
    /// Create a GitHub App installation access token with an explicit scope and permission set.
    pub async fn create_installation_access_token(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        #[derive(Serialize)]
        struct CreateInstallationAccessTokenBody {
            #[serde(skip_serializing_if = "Option::is_none")]
            repositories: Option<Vec<String>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            repository_ids: Option<Vec<u64>>,
            permissions: HashMap<String, String>,
        }

        #[derive(Deserialize)]
        struct RawInstallationAccessToken {
            token: String,
            expires_at: String,
        }

        let request: InstallationAccessTokenRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let permissions = HashMap::from([(
            permission_key(request.permission.key).to_string(),
            permission_level(request.permission.value).to_string(),
        )]);

        let (repositories, repository_ids, scope_label) = match request.scope {
            InstallationAccessTokenScope::AllRepositories => (None, None, "all"),
            InstallationAccessTokenScope::SelectedRepositories { repositories } => (
                Some(validate_repository_scope(self, repositories)?),
                None,
                "repositories",
            ),
            InstallationAccessTokenScope::SelectedRepositoryIds { repository_ids } => (
                None,
                Some(validate_repository_id_scope(repository_ids)?),
                "repository_ids",
            ),
        };

        let (client, installation_id) = build_github_app_client(&self.config.authentication)?;
        let address = format!("/app/installations/{installation_id}/access_tokens");
        info!(
            "Creating a GitHub installation access token with {scope_label} scope on behalf of {module}"
        );

        let request = CreateInstallationAccessTokenBody {
            repositories,
            repository_ids,
            permissions,
        };

        match client._post(address, Some(&request)).await {
            Ok(response) => {
                let status = response.status().as_u16();
                let body = client.body_to_string(response).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                })?;

                if status != 201 {
                    return Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )));
                }

                let token: RawInstallationAccessToken = serde_json::from_str(&body)
                    .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;
                serde_json::to_string(&InstallationAccessToken {
                    token: token.token,
                    expires_at: token.expires_at,
                })
                .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }
}

fn permission_key(key: InstallationAccessTokenPermissionKey) -> &'static str {
    match key {
        InstallationAccessTokenPermissionKey::Contents => "contents",
    }
}

fn permission_level(level: InstallationAccessTokenPermissionValue) -> &'static str {
    match level {
        InstallationAccessTokenPermissionValue::Read => "read",
        InstallationAccessTokenPermissionValue::Write => "write",
    }
}

fn validate_repository_scope(
    github: &Github,
    repositories: Vec<String>,
) -> Result<Vec<String>, ApiError> {
    if repositories.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let mut validated = Vec::new();
    for full_name in repositories {
        github.validate_repository_name(&full_name)?;

        let mut parts = full_name.split('/');
        let owner = parts.next().ok_or(ApiError::BadRequest)?;
        let repo = parts.next().ok_or(ApiError::BadRequest)?;
        if parts.next().is_some() {
            return Err(ApiError::BadRequest);
        }

        github.validate_org(owner)?;
        github.validate_repository_name(repo)?;
        if !validated.iter().any(|existing| existing == repo) {
            validated.push(repo.to_string());
        }
    }

    Ok(validated)
}

fn validate_repository_id_scope(repository_ids: Vec<u64>) -> Result<Vec<u64>, ApiError> {
    if repository_ids.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let mut validated = Vec::new();
    for repository_id in repository_ids {
        if repository_id == 0 || validated.contains(&repository_id) {
            if repository_id == 0 {
                return Err(ApiError::BadRequest);
            }
            continue;
        }

        validated.push(repository_id);
    }

    Ok(validated)
}
