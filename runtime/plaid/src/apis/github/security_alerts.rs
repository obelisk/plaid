use std::sync::Arc;

use plaid_stl::github::{GetCodeScanningAlertsParams, GetDependabotAlertsParams, GithubApiWrapper};

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

impl Github {
    /// List the Dependabot alerts for a repository.
    ///
    /// This endpoint is cursor-paginated: GitHub does not accept a `page` parameter, so the
    /// next page is identified by the `Link` response header. Rather than parsing that header
    /// ourselves, we deserialize into `octocrab::Page`, which already extracts the `next` URI
    /// from it. The next page is then fetched by following that URI verbatim, so the cursor is
    /// never re-encoded or interpreted by us.
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

        // The next page URL comes from a previous response's Link header, so it is opaque.
        // We must not blindly follow it: a module could pass an arbitrary URL and turn this
        // into a request to an unintended host. We therefore only accept a URL that points at
        // the Dependabot alerts endpoint for the same repository.
        let address = match request.params.next_page.as_deref() {
            Some(next_page) => {
                validate_next_page_url(next_page, owner, repo)?;
                next_page.to_string()
            }
            None => {
                let mut address =
                    format!("/repos/{owner}/{repo}/dependabot/alerts?per_page={per_page}");

                // When only open alerts are requested we let GitHub do the filtering, which
                // also means we do not have to page through dismissed/fixed alerts.
                if request.params.only_open {
                    address.push_str("&state=open");
                }

                address
            }
        };

        info!("Fetching Dependabot alerts for repo [{owner}/{repo}] on behalf of {module}");

        let client = self.clients.get(&request.client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                request.client_id
            )))
        })?;

        // `Page` deserializes the body into `items` and parses the `Link` header into
        // `next`/`prev`/`first`/`last`. Octocrab maps non-2xx responses to an error.
        let page: octocrab::Page<serde_json::Value> = client
            .get(address, None::<&()>)
            .await
            .map_err(|e| ApiError::GitHubError(GitHubError::ClientError(e)))?;

        // The alerts are passed through verbatim, so we only need to wrap them together with
        // the URL of the next page, if there is one.
        let response = serde_json::json!({
            "alerts": page.items,
            "next_page": page.next.map(|uri| uri.to_string()),
        });

        Ok(response.to_string())
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

/// Validate a `next_page` URL supplied by a module.
///
/// The URL originates from a previous response's `Link` header, but it is round-tripped
/// through the module, so it must be treated as untrusted input. We only accept an `https`
/// URL on `api.github.com` whose path is the Dependabot alerts endpoint for the same
/// repository that was requested. This prevents a module from using this function to make
/// requests to arbitrary hosts or to read alerts for a different repository.
fn validate_next_page_url(next_page: &str, owner: &str, repo: &str) -> Result<(), ApiError> {
    let expected_path = format!("/repos/{owner}/{repo}/dependabot/alerts");
    let url = reqwest::Url::parse(next_page).map_err(|_| ApiError::BadRequest)?;

    if url.scheme() != "https"
        || url.host_str() != Some("api.github.com")
        || url.path() != expected_path
    {
        return Err(ApiError::BadRequest);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_next_page_url;

    const OK: &str = "https://api.github.com/repos/o/r/dependabot/alerts?per_page=100&after=abc";

    #[test]
    fn accepts_url_for_same_repo() {
        assert!(validate_next_page_url(OK, "o", "r").is_ok());
    }

    #[test]
    fn accepts_cursor_containing_plus_and_slash() {
        // Base64 cursors can contain '+' and '/', which arrive percent-encoded.
        let url =
            "https://api.github.com/repos/o/r/dependabot/alerts?per_page=100&after=%2B%2F%2B%2B";
        assert!(validate_next_page_url(url, "o", "r").is_ok());
    }

    #[test]
    fn rejects_other_host() {
        let url = "https://evil.example.com/repos/o/r/dependabot/alerts?after=abc";
        assert!(validate_next_page_url(url, "o", "r").is_err());
    }

    #[test]
    fn rejects_plain_http() {
        let url = "http://api.github.com/repos/o/r/dependabot/alerts?after=abc";
        assert!(validate_next_page_url(url, "o", "r").is_err());
    }

    #[test]
    fn rejects_other_repository() {
        let url = "https://api.github.com/repos/o/other/dependabot/alerts?after=abc";
        assert!(validate_next_page_url(url, "o", "r").is_err());
    }

    #[test]
    fn rejects_other_endpoint() {
        let url = "https://api.github.com/repos/o/r/code-scanning/alerts?after=abc";
        assert!(validate_next_page_url(url, "o", "r").is_err());
    }

    #[test]
    fn rejects_path_traversal_attempt() {
        let url = "https://api.github.com/repos/o/r/dependabot/alerts/../../../../user";
        assert!(validate_next_page_url(url, "o", "r").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(validate_next_page_url("not a url", "o", "r").is_err());
    }
}
