use std::sync::Arc;

use plaid_stl::github::{GetCodeScanningAlertsParams, GetDependabotAlertsParams, GithubApiWrapper};

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::{extract_next_cursor, Github};

impl Github {
    /// List the Dependabot alerts for a repository.
    ///
    /// This endpoint is cursor-paginated: GitHub does not accept a `page` parameter, so the
    /// next page is identified by the `after` cursor found in the `Link` response header.
    /// The cursor is returned to the caller alongside the alerts so that the STL can walk
    /// through all the pages.
    ///
    /// See https://docs.github.com/en/rest/dependabot/alerts?apiVersion=2026-03-10#list-dependabot-alerts-for-a-repository
    pub async fn get_dependabot_alerts(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetDependabotAlertsParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        let per_page = request.params.per_page.unwrap_or(100);
        if per_page == 0 || per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        let mut address = format!("/repos/{owner}/{repo}/dependabot/alerts?per_page={per_page}");

        // When only open alerts are requested we let GitHub do the filtering, which also
        // means we do not have to page through dismissed/fixed alerts.
        if request.params.only_open {
            address.push_str("&state=open");
        }

        // The cursor is opaque and comes from a previous response's Link header, so we only
        // need to make sure it cannot be used to alter the request in unexpected ways.
        if let Some(cursor) = request.params.after.as_deref() {
            if !cursor
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '=' || c == '-' || c == '_')
            {
                return Err(ApiError::BadRequest);
            }
            address.push_str("&after=");
            address.push_str(cursor);
        }

        info!("Fetching Dependabot alerts for repo [{owner}/{repo}] on behalf of {module}");

        match self
            .make_generic_get_request_with_link_header(&request.client_id, address, module)
            .await
        {
            Ok((status, link, Ok(body))) => {
                if status != 200 {
                    return Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )));
                }

                let next_cursor = link.as_deref().and_then(extract_next_cursor);

                // The alerts are passed through verbatim, so we only need to wrap them
                // together with the cursor for the next page.
                let response = serde_json::json!({
                    "alerts": serde_json::from_str::<serde_json::Value>(&body)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?,
                    "next_cursor": next_cursor,
                });

                Ok(response.to_string())
            }
            Ok((_, _, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    /// List the code scanning alerts for a repository.
    ///
    /// Unlike Dependabot alerts, this endpoint supports a `page` parameter, so pagination is
    /// handled by the caller simply incrementing the page number.
    ///
    /// See https://docs.github.com/en/rest/code-scanning/code-scanning?apiVersion=2026-03-10#list-code-scanning-alerts-for-a-repository
    pub async fn get_code_scanning_alerts(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<GetCodeScanningAlertsParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;

        let per_page = request.params.per_page.unwrap_or(100);
        if per_page == 0 || per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        let page = request.params.page.unwrap_or(1);
        if page == 0 {
            return Err(ApiError::BadRequest);
        }

        let mut address =
            format!("/repos/{owner}/{repo}/code-scanning/alerts?per_page={per_page}&page={page}");

        // When only open alerts are requested we let GitHub do the filtering, which also
        // means we do not have to page through dismissed/fixed alerts.
        if request.params.only_open {
            address.push_str("&state=open");
        }

        info!("Fetching code scanning alerts for repo [{owner}/{repo}] on behalf of {module}");

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
}
