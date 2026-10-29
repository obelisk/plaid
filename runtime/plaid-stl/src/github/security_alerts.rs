use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{CodeScanningAlert, DependabotAlert, GithubApiWrapper},
    PlaidFunctionError,
};

/// Parameters sent to the runtime when listing Dependabot alerts for a repository.
#[derive(Serialize, Deserialize)]
pub struct GetDependabotAlertsParams {
    pub owner: String,
    pub repo: String,
    /// When true, only open alerts are returned.
    pub only_open: bool,
    /// The number of results per page (max 100).
    pub per_page: Option<u8>,
    /// The cursor identifying the page to fetch. This is opaque and is obtained from a
    /// previous call; rule authors should use `get_dependabot_alerts` instead of setting it.
    pub after: Option<String>,
}

/// Parameters sent to the runtime when listing code scanning alerts for a repository.
#[derive(Serialize, Deserialize)]
pub struct GetCodeScanningAlertsParams {
    pub owner: String,
    pub repo: String,
    /// When true, only open alerts are returned.
    pub only_open: bool,
    /// The number of results per page (max 100).
    pub per_page: Option<u8>,
    /// The page number of the results to fetch.
    pub page: Option<u16>,
}

/// The response returned by the runtime when listing Dependabot alerts.
#[derive(Deserialize)]
struct DependabotAlertsPage {
    alerts: Vec<DependabotAlert>,
    next_cursor: Option<String>,
}

/// Get all the Dependabot alerts for a repository.
///
/// This function transparently walks through all the pages of results, so the returned
/// vector contains every alert that matches the request.
///
/// See https://docs.github.com/en/rest/dependabot/alerts?apiVersion=2026-03-10#list-dependabot-alerts-for-a-repository
///
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `only_open` - When true, only open alerts are returned.
///
/// ## Returns
/// * `Ok(Vec<DependabotAlert>)` with all matching alerts, or
/// * `Err(PlaidFunctionError)` if the request fails.
pub fn get_dependabot_alerts(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    only_open: bool,
) -> Result<Vec<DependabotAlert>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_dependabot_alerts);
    }

    // Use the largest page size to make as few requests as possible.
    let per_page = 100;

    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

    let mut alerts = Vec::<DependabotAlert>::new();
    let mut after: Option<String> = None;

    loop {
        let params = GetDependabotAlertsParams {
            owner: owner.to_string(),
            repo: repo.to_string(),
            only_open,
            per_page: Some(per_page),
            after: after.clone(),
        };

        let wrapper = GithubApiWrapper {
            client_id: client_id.to_string(),
            params,
        };

        let request = serde_json::to_string(&wrapper).unwrap(); // safe unwrap

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_get_dependabot_alerts(
                request.as_bytes().as_ptr(),
                request.as_bytes().len(),
                return_buffer.as_mut_ptr(),
                RETURN_BUFFER_SIZE,
            )
        };

        if res < 0 {
            return Err(res.into());
        }

        return_buffer.truncate(res as usize);
        // This should be safe because unless the Plaid runtime is expressly trying
        // to mess with us, this came from a String in the API module.
        let this_page = String::from_utf8(return_buffer).unwrap();
        let this_page = serde_json::from_str::<DependabotAlertsPage>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        alerts.extend(this_page.alerts);

        // If GitHub did not give us a cursor for the next page then we are done.
        match this_page.next_cursor {
            Some(cursor) => after = Some(cursor),
            None => break,
        }
    }

    Ok(alerts)
}

/// Get all the code scanning alerts for a repository.
///
/// This function transparently walks through all the pages of results, so the returned
/// vector contains every alert that matches the request.
///
/// See https://docs.github.com/en/rest/code-scanning/code-scanning?apiVersion=2026-03-10#list-code-scanning-alerts-for-a-repository
///
/// ## Arguments
/// * `client_id` - Selects which configured GitHub client to use (supports multiple clients).
/// * `owner` - The account owner of the repository. The name is not case sensitive.
/// * `repo` - The name of the repository without the .git extension. The name is not case sensitive.
/// * `only_open` - When true, only open alerts are returned.
///
/// ## Returns
/// * `Ok(Vec<CodeScanningAlert>)` with all matching alerts, or
/// * `Err(PlaidFunctionError)` if the request fails.
pub fn get_code_scanning_alerts(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
    only_open: bool,
) -> Result<Vec<CodeScanningAlert>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_code_scanning_alerts);
    }

    // Use the largest page size to make as few requests as possible.
    let per_page = 100;

    const RETURN_BUFFER_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

    let mut alerts = Vec::<CodeScanningAlert>::new();
    let mut page: u16 = 1;

    loop {
        let params = GetCodeScanningAlertsParams {
            owner: owner.to_string(),
            repo: repo.to_string(),
            only_open,
            per_page: Some(per_page),
            page: Some(page),
        };

        let wrapper = GithubApiWrapper {
            client_id: client_id.to_string(),
            params,
        };

        let request = serde_json::to_string(&wrapper).unwrap(); // safe unwrap

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_get_code_scanning_alerts(
                request.as_bytes().as_ptr(),
                request.as_bytes().len(),
                return_buffer.as_mut_ptr(),
                RETURN_BUFFER_SIZE,
            )
        };

        if res < 0 {
            return Err(res.into());
        }

        return_buffer.truncate(res as usize);
        // This should be safe because unless the Plaid runtime is expressly trying
        // to mess with us, this came from a String in the API module.
        let this_page = String::from_utf8(return_buffer).unwrap();
        let this_page = serde_json::from_str::<Vec<CodeScanningAlert>>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        let alerts_returned = this_page.len();
        alerts.extend(this_page);

        // If we did not fill this page, we know there won't be a next one.
        if alerts_returned < per_page as usize {
            break;
        }

        page += 1;
    }

    Ok(alerts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed-down but structurally faithful Dependabot alert, as returned by
    /// GET /repos/{owner}/{repo}/dependabot/alerts
    const DEPENDABOT_ALERT: &str = r#"{
        "number": 1,
        "state": "open",
        "dependency": {
            "package": { "ecosystem": "npm", "name": "lodash" },
            "manifest_path": "package-lock.json",
            "scope": "runtime",
            "relationship": "direct"
        },
        "security_advisory": {
            "ghsa_id": "GHSA-35jh-r3h4-6jhm",
            "cve_id": "CVE-2021-23337",
            "summary": "Command Injection in lodash",
            "description": "A long description we do not parse.",
            "severity": "high",
            "identifiers": [
                { "type": "GHSA", "value": "GHSA-35jh-r3h4-6jhm" },
                { "type": "CVE", "value": "CVE-2021-23337" }
            ],
            "published_at": "2021-02-15T19:24:22Z",
            "updated_at": "2022-01-21T23:22:15Z"
        },
        "security_vulnerability": {
            "package": { "ecosystem": "npm", "name": "lodash" },
            "severity": "high",
            "vulnerable_version_range": "< 4.17.21",
            "first_patched_version": { "identifier": "4.17.21" }
        },
        "url": "https://api.github.com/repos/o/r/dependabot/alerts/1",
        "html_url": "https://github.com/o/r/security/dependabot/1",
        "created_at": "2021-03-01T00:00:00Z",
        "updated_at": "2021-03-02T00:00:00Z",
        "dismissed_at": null,
        "dismissed_reason": null,
        "fixed_at": null
    }"#;

    /// A trimmed-down but structurally faithful code scanning alert, as returned by
    /// GET /repos/{owner}/{repo}/code-scanning/alerts
    const CODE_SCANNING_ALERT: &str = r#"{
        "number": 4,
        "state": "open",
        "rule": {
            "id": "js/unused-local-variable",
            "name": "js/unused-local-variable",
            "severity": "note",
            "security_severity_level": null,
            "description": "Unused variable, import, function or class"
        },
        "tool": { "name": "CodeQL", "version": "2.11.5", "guid": null },
        "most_recent_instance": {
            "ref": "refs/heads/main",
            "state": "open",
            "commit_sha": "abc123",
            "location": {
                "path": "src/index.js",
                "start_line": 10,
                "end_line": 10,
                "start_column": 7,
                "end_column": 12
            }
        },
        "url": "https://api.github.com/repos/o/r/code-scanning/alerts/4",
        "html_url": "https://github.com/o/r/security/code-scanning/4",
        "created_at": "2022-06-01T00:00:00Z",
        "updated_at": "2022-06-02T00:00:00Z",
        "dismissed_at": null,
        "dismissed_reason": null,
        "fixed_at": null
    }"#;

    #[test]
    fn deserializes_dependabot_alert() {
        let alert: DependabotAlert = serde_json::from_str(DEPENDABOT_ALERT).unwrap();
        assert_eq!(alert.number, 1);
        assert_eq!(alert.state, crate::github::DependabotAlertState::Open);
        assert_eq!(alert.dependency.package.name, "lodash");
        assert_eq!(alert.dependency.package.ecosystem, "npm");
        assert_eq!(
            alert.security_advisory.cve_id.as_deref(),
            Some("CVE-2021-23337")
        );
        assert_eq!(alert.security_advisory.ghsa_id, "GHSA-35jh-r3h4-6jhm");
        assert_eq!(
            alert.security_advisory.severity,
            crate::github::AdvisorySeverity::High
        );
        assert_eq!(alert.security_advisory.identifiers.len(), 2);
        assert_eq!(
            alert
                .security_vulnerability
                .first_patched_version
                .as_ref()
                .unwrap()
                .identifier,
            "4.17.21"
        );
        assert!(alert.dismissed_at.is_none());
        assert!(alert.fixed_at.is_none());
    }

    #[test]
    fn deserializes_dependabot_alert_without_cve() {
        // Malware advisories and some GHSA-only advisories have no CVE.
        let json = DEPENDABOT_ALERT.replace(r#""cve_id": "CVE-2021-23337","#, r#""cve_id": null,"#);
        let alert: DependabotAlert = serde_json::from_str(&json).unwrap();
        assert!(alert.security_advisory.cve_id.is_none());
    }

    #[test]
    fn deserializes_code_scanning_alert() {
        let alert: CodeScanningAlert = serde_json::from_str(CODE_SCANNING_ALERT).unwrap();
        assert_eq!(alert.number, 4);
        assert_eq!(alert.state, crate::github::CodeScanningAlertState::Open);
        assert_eq!(alert.rule.name, "js/unused-local-variable");
        assert!(alert.rule.security_severity_level.is_none());
        assert_eq!(alert.tool.name, "CodeQL");
        let instance = alert.most_recent_instance.as_ref().unwrap();
        assert_eq!(instance.r#ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(instance.location.as_ref().unwrap().path, "src/index.js");
    }

    #[test]
    fn deserializes_dependabot_page_with_cursor() {
        let json = format!(r#"{{"alerts": [{DEPENDABOT_ALERT}], "next_cursor": "abc"}}"#);
        let page: DependabotAlertsPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page.alerts.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("abc"));
    }

    #[test]
    fn deserializes_dependabot_page_without_cursor() {
        let json = format!(r#"{{"alerts": [{DEPENDABOT_ALERT}], "next_cursor": null}}"#);
        let page: DependabotAlertsPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page.alerts.len(), 1);
        assert!(page.next_cursor.is_none());
    }
}
