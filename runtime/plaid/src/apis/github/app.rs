use std::{collections::HashMap, sync::Arc};

use plaid_stl::github::{
    InstallationAccessToken, InstallationAccessTokenPermissionKey,
    InstallationAccessTokenPermissionValue, InstallationAccessTokenRequest,
    InstallationAccessTokenScope,
};
use serde::{Deserialize, Serialize};

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

        #[derive(Deserialize)]
        struct RawInstallationAccessToken {
            token: String,
            expires_at: String,
        }

        let request: InstallationAccessTokenRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let installation_id = if let Authentication::App {
            installation_id, ..
        } = self.config.authentication
        {
            installation_id
        } else {
            return Err(ApiError::ConfigurationError(
                "Github App is required for creating installation access token".to_string(),
            ));
        };

        let (repositories, repository_ids) = match request.scope {
            InstallationAccessTokenScope::AllRepositories => {
                info!(
                    "Creating a GitHub installation access token with [all_repositories] scope and [{}] permissions on behalf of {module}", request.permissions,
                );
                (None, None)
            }
            InstallationAccessTokenScope::SelectedRepositories { repositories } => {
                info!(
                    "Creating a GitHub installation access token with [{repositories}] scope and [{}] permissions on behalf of {module}", request.permissions,
                );

                (Some(validate_repository_scope(self, repositories)?), None)
            }
            InstallationAccessTokenScope::SelectedRepositoryIds { repository_ids } => {
                info!(
                    "Creating a GitHub installation access token with [{repository_ids}] scope and [{}] permissions on behalf of {module}", request.permissions,
                );

                (None, Some(validate_repository_id_scope(repository_ids)?))
            }
        };

        let body = CreateInstallationAccessTokenBody {
            repositories,
            repository_ids,
            permissions: request.permissions.into(),
        };

        let address = format!("/app/installations/{installation_id}/access_tokens");

        match self.make_generic_post_request(address, &body, module).await {
            Ok((status, Ok(body))) => {
                if status == 201 {
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
