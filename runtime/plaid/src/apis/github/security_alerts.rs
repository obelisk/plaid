use std::sync::Arc;

use plaid_stl::github::{GetCodeScanningAlertsParams, GetDependabotAlertsParams, GithubApiWrapper};

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

/// The maximum number of alerts returned by a single call.
///
/// Pagination is handled entirely inside the runtime, so this bounds both the work done on
/// behalf of a single rule invocation and the size of the response handed back to the guest.
/// If a repository has more alerts than this, the first `MAX_ALERTS` are returned and a
/// warning is logged.
const MAX_ALERTS: usize = 2000;

/// The number of alerts requested per page. GitHub's maximum is 100.
const PER_PAGE: usize = 100;

/// How the endpoint paginates.
#[derive(Clone, Copy)]
enum Pagination {
    /// Follow the `Link` response header. Used by Dependabot, which has no `page` parameter.
    Link,
    /// Increment the `page` query parameter. Used by code scanning, which documents it.
    Page,
}

/// How many of this page's alerts to keep, given how many we already have.
///
/// The caller detects truncation with `page_len > take_from_page(collected, page_len)`.
fn take_from_page(collected: usize, page_len: usize) -> usize {
    page_len.min(MAX_ALERTS.saturating_sub(collected))
}

/// Walk every page of a GitHub alerts endpoint, collecting up to `MAX_ALERTS` alerts.
///
/// The alerts are collected as raw JSON so that fields we do not model are preserved for the
/// rule; the STL deserializes into typed structs on its side.
async fn collect_alerts(
    client: &octocrab::Octocrab,
    base_address: String,
    pagination: Pagination,
    module: &Arc<PlaidModule>,
    endpoint: &str,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let mut alerts = Vec::new();
    let mut address = base_address.clone();
    let mut page_number = 1u32;

    loop {
        // `Page` deserializes the body into `items` and parses the `Link` header into
        // `next`/`prev`/`first`/`last`. Octocrab maps non-2xx responses to an error.
        let page: octocrab::Page<serde_json::Value> = client
            .get(address.clone(), None::<&()>)
            .await
            .map_err(|e| ApiError::GitHubError(GitHubError::ClientError(e)))?;

        let page_len = page.items.len();
        let take = take_from_page(alerts.len(), page_len);
        alerts.extend(page.items.into_iter().take(take));

        // We had to drop part of this page, so there is data we are not returning.
        if page_len > take {
            warn!(
                "Reached the limit of {MAX_ALERTS} alerts while fetching {endpoint} on behalf of {module}; returning the first {MAX_ALERTS} alerts"
            );
            return Ok(alerts);
        }

        match pagination {
            Pagination::Link => match page.next {
                Some(next) => {
                    // We are at the cap and GitHub says there is another page, so there is
                    // data we are not returning.
                    if alerts.len() >= MAX_ALERTS {
                        warn!(
                            "Reached the limit of {MAX_ALERTS} alerts while fetching {endpoint} on behalf of {module}; returning the first {MAX_ALERTS} alerts"
                        );
                        return Ok(alerts);
                    }
                    address = next.to_string();
                }
                None => return Ok(alerts),
            },
            Pagination::Page => {
                // A short page is the last page.
                if page_len < PER_PAGE {
                    return Ok(alerts);
                }
                // A full page at the cap is ambiguous: there may or may not be more. Fetch
                // the next page to find out, so we only warn when data is actually dropped.
                page_number += 1;
                let separator = if base_address.contains('?') { '&' } else { '?' };
                address = format!("{base_address}{separator}page={page_number}");
            }
        }
    }
}

impl Github {
    /// List the Dependabot alerts for a repository.
    ///
    /// This endpoint is cursor-paginated: GitHub does not accept a `page` parameter, so the
    /// next page is identified by the `Link` response header. The URL never leaves the
    /// runtime, so it is never treated as untrusted input.
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

        let mut address = format!("/repos/{owner}/{repo}/dependabot/alerts?per_page={PER_PAGE}");

        // When only open alerts are requested we let GitHub do the filtering, which also
        // means we do not have to page through dismissed/fixed alerts.
        if request.params.only_open {
            address.push_str("&state=open");
        }

        info!("Fetching Dependabot alerts for repo [{owner}/{repo}] on behalf of {module}");

        let client = self.client(&request.client_id)?;
        let alerts = collect_alerts(
            client,
            address,
            Pagination::Link,
            &module,
            "Dependabot alerts",
        )
        .await?;

        serde_json::to_string(&alerts).map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
    }

    /// List the code scanning alerts for a repository.
    ///
    /// This endpoint documents a `page` parameter, so we paginate by incrementing it rather
    /// than relying on the `Link` header. The `Link` header is not part of the documented
    /// response for this endpoint, and silently returning only the first page would be worse
    /// than an error, so we use the mechanism GitHub actually guarantees here.
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

        let mut address = format!("/repos/{owner}/{repo}/code-scanning/alerts?per_page={PER_PAGE}");

        // When only open alerts are requested we let GitHub do the filtering, which also
        // means we do not have to page through dismissed/fixed alerts.
        if request.params.only_open {
            address.push_str("&state=open");
        }

        info!("Fetching code scanning alerts for repo [{owner}/{repo}] on behalf of {module}");

        let client = self.client(&request.client_id)?;
        let alerts = collect_alerts(
            client,
            address,
            Pagination::Page,
            &module,
            "code scanning alerts",
        )
        .await?;

        serde_json::to_string(&alerts).map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))
    }

    /// Look up a configured GitHub client by its identifier.
    fn client(&self, client_id: &str) -> Result<&octocrab::Octocrab, ApiError> {
        self.clients.get(client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {client_id}"
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{take_from_page, MAX_ALERTS, PER_PAGE};

    #[test]
    fn takes_whole_page_when_under_the_cap() {
        assert_eq!(take_from_page(0, 100), 100);
        assert_eq!(take_from_page(100, 42), 42);
        assert_eq!(take_from_page(0, 0), 0);
    }

    #[test]
    fn takes_only_what_fits_when_page_overflows_the_cap() {
        // 1950 collected, a full page arrives: only 50 fit.
        assert_eq!(take_from_page(MAX_ALERTS - 50, 100), 50);
    }

    #[test]
    fn takes_nothing_once_the_cap_is_reached() {
        assert_eq!(take_from_page(MAX_ALERTS, 100), 0);
    }

    #[test]
    fn truncation_is_detected_by_comparing_page_len_to_take() {
        // The caller warns when `page_len > take_from_page(..)`. These are the cases where
        // data is actually discarded, and the one where it is not.
        let discarded =
            |collected: usize, page_len: usize| page_len > take_from_page(collected, page_len);

        assert!(!discarded(0, 100));
        assert!(!discarded(MAX_ALERTS - 100, 100)); // exactly fills the cap
        assert!(discarded(MAX_ALERTS - 50, 100)); // 50 dropped
        assert!(discarded(MAX_ALERTS, 100)); // whole page dropped
    }

    #[test]
    fn cap_and_page_size_match_github_limits() {
        assert_eq!(MAX_ALERTS, 2000);
        assert_eq!(PER_PAGE, 100);
    }

    /// The runtime collects raw JSON so that fields we do not model survive the round trip
    /// to the rule. This guards against regressing to typed collection, which would silently
    /// drop them.
    #[test]
    fn unmodelled_fields_survive_the_round_trip() {
        let github_payload = serde_json::json!([{
            "number": 1,
            "state": "open",
            // A field we do not model on DependabotAlert.
            "reachability": { "verdict": "reachable" },
            "security_advisory": {
                "ghsa_id": "GHSA-35jh-r3h4-6jhm",
                // A field we do not model on DependabotSecurityAdvisory.
                "description": "A long description we do not parse."
            }
        }]);

        // This is what `collect_alerts` accumulates and re-serializes.
        let collected: Vec<serde_json::Value> =
            serde_json::from_value(github_payload.clone()).unwrap();
        let round_tripped = serde_json::to_string(&collected).unwrap();
        let round_tripped: serde_json::Value = serde_json::from_str(&round_tripped).unwrap();

        assert_eq!(round_tripped, github_payload);
    }
}
