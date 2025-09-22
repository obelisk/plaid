use std::sync::Arc;

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use plaid_stl::github::{GetOrCreateBranchReferenceParams, GitRef};

impl Github {
    /// Returns a single reference from the Git database.
    pub async fn get_reference(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GetOrCreateBranchReferenceParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;

        // Validate the reference. In practice, tags and branches follow the same naming conventions
        // so we'll use the same validator.
        match &request.reference {
            GitRef::Branch(name) | GitRef::Tag(name) => self.validate_branch_name(name)?,
        };

        info!(
            "Fetching reference [{}] for repository [{owner}/{repo}] on behalf of [{module}]",
            request.reference
        );
        let address = format!("/repos/{owner}/{repo}/git/ref/{}", request.reference);

        match self.make_generic_get_request(address, module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else if status == 404 {
                    Ok(Default::default())
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

    /// Creates a reference for a repository.
    pub async fn create_reference(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GetOrCreateBranchReferenceParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let Some(sha) = request.sha else {
            return Err(ApiError::BadRequest);
        };

        let owner = self.validate_username(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;
        let sha = self.validate_commit_hash(&sha)?;

        let reference = format!("refs/{}", request.reference);

        info!(
            "Creating reference [{reference}] for repository [{owner}/{repo}] on behalf of [{module}]",
        );

        let address = format!("/repos/{owner}/{repo}/git/refs");

        let body = serde_json::json!({
            "ref": reference,
            "sha": sha,
        });

        match self.make_generic_post_request(address, body, module).await {
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
