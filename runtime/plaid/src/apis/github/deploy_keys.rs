use std::sync::Arc;

use plaid_stl::github::DeleteDeployKeyParams;

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
        let request: DeleteDeployKeyParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let owner = self.validate_username(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;
        let key_id = request.key_id; // this is implicitly validated because it's a u64

        info!("Removing deploy key with ID {key_id} from repo {owner}/{repo}");

        let address = format!("/repos/{owner}/{repo}/keys/{key_id}");

        match self
            .make_generic_delete_request(address, None::<&String>, module)
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
}
