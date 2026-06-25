use std::sync::Arc;

use plaid_stl::github::{
    ConsumedLicensesResponse, GithubApiWrapper, GrantRepoAccessToOrgInstallationParams,
    RemoveRepoAccessFromOrgInstallationParams,
};
use serde_json::json;

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// Grant a GitHub organization installation access to one or more repositories
    /// See https://docs.github.com/en/enterprise-cloud@latest/rest/enterprise-admin/organization-installations?apiVersion=2026-03-10#grant-repository-access-to-an-organization-installation for more detail
    pub async fn grant_repo_access_to_org_installation(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<GrantRepoAccessToOrgInstallationParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // We use the same validator for enterprise and org
        let enterprise = self.validate_org(&request.params.enterprise)?;
        let org = self.validate_org(&request.params.org)?;
        let installation_id = request.params.installation_id;

        for repo in &request.params.repositories {
            self.validate_repository_name(repo)?;
        }

        info!(
            "Granting repository access to organization installation [{installation_id}] for enterprise [{enterprise}] and organization [{org}] on behalf of [{module}]. Involved repos: {:?}",
            request.params.repositories
        );

        let address = format!("/enterprises/{enterprise}/apps/organizations/{org}/installations/{installation_id}/repositories/add");

        let body = json!({
            "repositories": request.params.repositories,
        });

        match self
            .make_generic_patch_request(&request.client_id, address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
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

    /// Removes a GitHub organization installation's access to one or more repositories
    /// See https://docs.github.com/en/enterprise-cloud@latest/rest/enterprise-admin/organization-installations?apiVersion=2026-03-10#remove-repository-access-from-an-organization-installation for more detail
    pub async fn remove_repo_access_from_org_installation(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<RemoveRepoAccessFromOrgInstallationParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // We use the same validator for enterprise and org
        let enterprise = self.validate_org(&request.params.enterprise)?;
        let org = self.validate_org(&request.params.org)?;
        let installation_id = request.params.installation_id;

        for repo in &request.params.repositories {
            self.validate_repository_name(repo)?;
        }

        info!(
            "Removing repository access from organization installation [{installation_id}] for enterprise [{enterprise}] and organization [{org}] on behalf of [{module}]. Involved repos: {:?}",
            request.params.repositories
        );

        let address = format!("/enterprises/{enterprise}/apps/organizations/{org}/installations/{installation_id}/repositories/remove");

        let body = json!({
            "repositories": request.params.repositories,
        });

        match self
            .make_generic_patch_request(&request.client_id, address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
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

    /// Get the number of consumed licenses for an enterprise
    /// ## Arguments
    /// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
    /// * `enterprise` - The enterprise name
    pub async fn get_enterprise_license_status(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let enterprise = self.validate_org(&request.params)?;

        info!("Getting license status for enterprise [{enterprise}] on behalf of [{module}]");

        // We don't really care about pagination because we are discarding information about users and
        // only extracting the licenses total counts, which are returned in the top-level JSON object.
        let address = format!("/enterprises/{enterprise}/consumed-licenses");

        // Instead of returning the full body to the rule, we parse it and only return info about total number of seats.
        // This is a bit awkward in the sense that we are deserializing and then reserializing, but it allows us to
        // avoid returning a lot of information that the rule doesn't need.
        let body = match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    body
                } else {
                    return Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )));
                }
            }
            Ok((_, Err(e))) => return Err(e),
            Err(e) => return Err(e),
        };

        let consumed_licenses: ConsumedLicensesResponse = serde_json::from_str(&body)
            .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;

        Ok(serde_json::to_string(&consumed_licenses)
            .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?)
    }
}
