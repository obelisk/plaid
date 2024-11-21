use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

impl Github {
    pub async fn list_fpat_requests_for_org(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;

        info!("Fetching FPAT Requests For {org} on behalf of {module}");
        let address = format!("/orgs/{org}/personal-access-token-requests");

        match self.make_generic_get_request(address, module).await {
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

    pub async fn get_repos_for_fpat(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let request_id =
            self.validate_pint(request.get("request_id").ok_or(ApiError::BadRequest)?)?;
        let per_page = self.validate_pint(request.get("per_page").unwrap_or(&"30"))?;
        let page = self.validate_pint(request.get("page").unwrap_or(&"1"))?;

        info!("Fetching Repos For FPAT {request_id} in {org} on behalf of {module}");
        let address =
            format!("/orgs/{org}/personal-access-token-requests/{request_id}/repositories?per_page={per_page}&page={page}");

        match self.make_generic_get_request(address, module).await {
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

    pub async fn review_fpat_requests_for_org(
        &self,
        params: &str,
        module: &str,
    ) -> Result<u32, ApiError> {
        #[derive(Deserialize, Serialize)]
        struct Request {
            #[serde(skip_serializing)]
            org: String,
            pat_request_ids: Vec<u64>,
            action: String,
            reason: String,
        }
        let request: Request = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let org = self.validate_org(&request.org)?;

        match request.action.as_str() {
            "approve" => {
                info!(
                    "Approving FPATs {:?} Requests For {org} on behalf of {module} because: {}",
                    request.pat_request_ids, request.reason,
                );
            }
            "deny" => {
                info!(
                    "Denying FPATs {:?} Requests For {org} on behalf of {module} because: {}",
                    request.pat_request_ids, request.reason,
                );
            }
            _ => {
                warn!(
                    "{module} tried to respond to PAT requests with an invalid action: {}",
                    request.action
                );
                return Err(ApiError::BadRequest);
            }
        }
        let address = format!("/orgs/{org}/personal-access-token-requests");

        match self
            .make_generic_post_request(address, &request, &module)
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
