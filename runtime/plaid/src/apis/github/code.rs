use std::sync::Arc;

use plaid_stl::github::{
    CreateInstallationAccessTokenParams, GithubApiWrapper, InstallationAccessTokenCreationResponse,
    SearchCodeParams,
};

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

impl Github {
    /// Search for files with given name.
    /// See https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#search-code for more details
    pub async fn search_code(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<SearchCodeParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        /*
        The request can contain the following parameters:
        - filename
        - extension
        - path
        - org
        - repo
        NOTE - All params are optional and we do not do any validation on whether the request as a whole makes sense.
        We simply validate individual parameters if present.
        */

        let file_content = request.params.file_content.clone().unwrap_or_default();

        if file_content.contains([':', '"', '\'', '(', ')', '&', '|', '\n', '\r', '\t']) {
            // These characters have special meaning in GitHub's search syntax and could be used for injection attacks, so we reject them outright.
            return Err(ApiError::BadRequest);
        }

        let filename = match request.params.filename {
            None => String::new(),
            Some(filename) => format!("filename:{}", self.validate_filename(&filename)?),
        };

        let extension = match request.params.extension {
            None => String::new(),
            Some(extension) => format!("extension:{}", self.validate_extension(&extension)?),
        };

        let path = match request.params.path {
            None => String::new(),
            Some(path) => format!("path:{}", self.validate_path(&path)?),
        };

        // See if we were told to search only in a specific organization or repository
        let org = match request.params.org {
            None => None,
            Some(org) => Some(self.validate_org(&org)?.to_string()),
        };
        let repo = match request.params.repo {
            None => None,
            Some(repo) => Some(self.validate_repository_name(&repo)?.to_string()),
        };

        // Assemble org and repo depending on what we received.
        // NOTE - If we received both, we need to set repo:{org}/{repo} in the query
        let org_and_repo = match (org, repo) {
            (None, None) => String::new(),
            (Some(org), Some(repo)) => format!("repo:{org}/{repo}"),
            (Some(org), _) => format!("org:{org}"),
            (_, Some(repo)) => format!("repo:{repo}"),
        };

        let per_page: u8 = request.params.per_page.unwrap_or(100);
        if per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        let page: u16 = request.params.page.unwrap_or(1);

        // Construct the query with the piece we have. Multiple spaces, if present, do not cause problems.
        let query = format!("{file_content} {filename} {extension} {path} {org_and_repo}");

        // Log what we are doing
        info!("Searching code in GH with query [{query}] on behalf of [{module}]");

        let query = urlencoding::encode(&query).to_string();

        // !!! NOTE - This endpoint has a custom rate limitation !!!
        // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
        let address = format!("/search/code?q={query}&per_page={per_page}&page={page}");

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

    /// Create an installation access token for a GitHub App installation.
    /// See https://docs.github.com/en/rest/apps/apps?apiVersion=2026-03-10#create-an-installation-access-token-for-an-app for more details
    /// Note that this endpoint is only available to GitHub Apps, and not to OAuth apps or personal access tokens.
    pub async fn create_installation_access_token(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<CreateInstallationAccessTokenParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let installation_id = &request.params.installation_id.to_string();

        info!(
            "Creating installation access token for installation [{installation_id}] on behalf of {module}"
        );

        let address = format!("/app/installations/{installation_id}/access_tokens");

        match self
            .make_app_authenticated_post_request(
                &request.client_id,
                address,
                &request.params.body,
                module,
            )
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 201 {
                    let response: InstallationAccessTokenCreationResponse =
                        serde_json::from_str(&body).map_err(|_| ApiError::BadRequest)?;
                    Ok(serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?)
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
