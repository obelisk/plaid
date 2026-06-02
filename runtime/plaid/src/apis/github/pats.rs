use std::sync::Arc;

use plaid_stl::github::{
    GetReposForFpatParams, GithubApiWrapper, ListFpatRequestsForOrgParams,
    ReviewFpatRequestsForOrgParams,
};
use serde::Serialize;

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// Fetch a list of all FPAT requests for a GitHub organization.
    pub async fn list_fpat_requests_for_org(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<ListFpatRequestsForOrgParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(&request.params.org)?;

        info!("Fetching FPAT Requests For {org} on behalf of {module}");
        let address = format!("/orgs/{org}/personal-access-token-requests");

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

    /// List the repositories a fine-grained personal access token request is requesting access to.
    pub async fn get_repos_for_fpat(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetReposForFpatParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(&request.params.org)?;
        let request_id = request.params.request_id;
        let per_page = request.params.per_page.unwrap_or(30);
        let page = request.params.page.unwrap_or(1);

        info!("Fetching Repos For FPAT {request_id} in {org} on behalf of {module}");
        let address =
            format!("/orgs/{org}/personal-access-token-requests/{request_id}/repositories?per_page={per_page}&page={page}");

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

    /// Approves or denies multiple pending requests to access organization resources via a fine-grained personal access token.
    pub async fn review_fpat_requests_for_org(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<ReviewFpatRequestsForOrgParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let org = self.validate_org(&request.params.org)?;

        match request.params.action.as_str() {
            "approve" => {
                info!(
                    "Approving FPATs {:?} Requests For {org} on behalf of {module} because: {}",
                    request.params.pat_request_ids, request.params.reason,
                );
            }
            "deny" => {
                info!(
                    "Denying FPATs {:?} Requests For {org} on behalf of {module} because: {}",
                    request.params.pat_request_ids, request.params.reason,
                );
            }
            _ => {
                warn!(
                    "{module} tried to respond to PAT requests with an invalid action: {}",
                    request.params.action
                );
                return Err(ApiError::BadRequest);
            }
        }
        let address = format!("/orgs/{org}/personal-access-token-requests");

        #[derive(Serialize)]
        struct RequestBody {
            pat_request_ids: Vec<u64>,
            action: String,
            reason: String,
        }

        let body = RequestBody {
            pat_request_ids: request.params.pat_request_ids,
            action: request.params.action,
            reason: request.params.reason,
        };

        match self
            .make_generic_post_request(&request.client_id, address, &body, module.clone())
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 202 {
                    Ok(0)
                } else {
                    warn!("{module} failed to review FPAT requests for {org}. Status Code: {status} Return was: {body}");
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
