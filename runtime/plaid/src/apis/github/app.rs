use std::{collections::HashMap, sync::Arc};

use http::header::USER_AGENT;
use jsonwebtoken::EncodingKey;
use octocrab::Octocrab;
use plaid_stl::github::{
    InstallationAccessTokenPermissionKey, InstallationAccessTokenPermissionValue,
    InstallationAccessTokenRequest, InstallationAccessTokenScope,
};
use serde::Serialize;

use crate::{
    apis::{
        github::{Authentication, GitHubError},
        ApiError,
    },
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// Create a GitHub App installation access token with an explicit scope and permission set.
    /// For more details, see https://docs.github.com/en/rest/apps/apps?apiVersion=2026-03-10#create-an-installation-access-token-for-an-app
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

        let request: InstallationAccessTokenRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let (installation_id, app_id, private_key) = if let Authentication::App {
            app_id,
            private_key,
            installation_id,
            ..
        } = &self.config.authentication
        {
            (*installation_id, *app_id, private_key)
        } else {
            return Err(ApiError::ConfigurationError(
                "Github App is required for creating installation access token".to_string(),
            ));
        };

        let InstallationAccessTokenRequest { scope, permissions } = request;

        let (repositories, repository_ids) = match scope {
            InstallationAccessTokenScope::AllRepositories => {
                info!(
                    "Creating a GitHub installation access token with [all_repositories] scope and {:?} permissions on behalf of {module}",
                    permissions,
                );
                (None, None)
            }
            InstallationAccessTokenScope::SelectedRepositories { repositories } => {
                info!(
                    "Creating a GitHub installation access token with {:?} scope and {:?} permissions on behalf of {module}",
                    repositories,
                    permissions,
                );

                let repositories = repositories
                    .into_iter()
                    .map(|repository| {
                        self.validate_repository_name(&repository)?;
                        Ok(repository)
                    })
                    .collect::<Result<Vec<_>, ApiError>>()?;

                (Some(repositories), None)
            }
            InstallationAccessTokenScope::SelectedRepositoryIds { repository_ids } => {
                info!(
                    "Creating a GitHub installation access token with {:?} scope and {:?} permissions on behalf of {module}",
                    repository_ids,
                    permissions,
                );

                let repository_ids = repository_ids
                    .into_iter()
                    .map(|repository_id| {
                        let repository_id_str = repository_id.to_string();
                        self.validate_repo_id(&repository_id_str)?;
                        Ok(repository_id)
                    })
                    .collect::<Result<Vec<_>, ApiError>>()?;

                (None, Some(repository_ids))
            }
        };

        let permissions = permissions
            .into_iter()
            .map(|(key, value)| {
                let key = match key {
                    InstallationAccessTokenPermissionKey::Contents => "contents",
                };
                let value = match value {
                    InstallationAccessTokenPermissionValue::Read => "read",
                    InstallationAccessTokenPermissionValue::Write => "write",
                };

                (key.to_string(), value.to_string())
            })
            .collect();

        let body = CreateInstallationAccessTokenBody {
            repositories,
            repository_ids,
            permissions,
        };

        let address = format!("/app/installations/{installation_id}/access_tokens");
        let app_client = Octocrab::builder()
            .app(
                app_id.into(),
                EncodingKey::from_rsa_pem(private_key.as_bytes()).map_err(|_| {
                    ApiError::ConfigurationError(
                        "Failed to create encoding key from private key for GitHub API".to_string(),
                    )
                })?,
            )
            .add_header(
                USER_AGENT,
                format!("Rust/Plaid{}", env!("CARGO_PKG_VERSION")),
            )
            .build()
            .map_err(|e| ApiError::GitHubError(GitHubError::ClientError(e)))?;

        match app_client._post(address, Some(&body)).await {
            Ok(response) => {
                let status = response.status().as_u16();
                let body = app_client.body_to_string(response).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                })?;

                if status == 201 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }
}
