use std::sync::Arc;

use plaid_stl::github::{CreateDeployKeyParams, DeleteDeployKeyParams, GithubApiWrapper};
use serde::Serialize;

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// Remove a deploy key with a given ID from a given repository.
    /// For more details, see https://docs.github.com/en/rest/deploy-keys/deploy-keys?apiVersion=2022-11-28#delete-a-deploy-key
    pub async fn delete_deploy_key(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<DeleteDeployKeyParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        let key_id = request.params.key_id; // this is implicitly validated because it's a u64

        info!("Removing deploy key with ID {key_id} from repo {owner}/{repo}");

        let address = format!("/repos/{owner}/{repo}/keys/{key_id}");

        match self
            .make_generic_delete_request(request.client_id, address, None::<&String>, module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 204 {
                    Ok(0)
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

    /// Create a deploy key with a given ID for a given repository.
    /// For more details, see https://docs.github.com/en/rest/deploy-keys/deploy-keys?apiVersion=2026-03-10#create-a-deploy-key
    pub async fn create_deploy_key(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<CreateDeployKeyParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        // The following are not validated because they are sent in the request body and will be validated by GitHub
        let title = &request.params.title;
        let key = &request.params.key;
        let read_only = request.params.read_only;

        let final_part = match read_only {
            true => "",
            false => "Warning: this deploy key will have write access to the repository.",
        };

        #[derive(Serialize)]
        struct Body {
            title: String,
            key: String,
            read_only: bool,
        }

        let body = Body {
            title: title.to_string(),
            key: key.to_string(),
            read_only,
        };

        info!("Creating deploy key with title [{title}] for repo {owner}/{repo}. {final_part}");

        let address = format!("/repos/{owner}/{repo}/keys");

        match self
            .make_generic_post_request(request.client_id, address, Some(&body), module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 201 {
                    Ok(0)
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
