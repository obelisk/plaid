use http::{HeaderMap, StatusCode};

/// The GitHub API resource a rate-limit window applies to. A closed enum so
/// header input cannot grow the metric label set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateLimitResource {
    Core,
    Search,
    Graphql,
    IntegrationManifest,
    CodeScanningUpload,
    AuditLog,
    SourceImport,
    DependencySnapshots,
    CodeSearch,
    ActionsRunnerRegistration,
    Scim,
    Other,
    Unknown,
}

impl RateLimitResource {
    fn from_header(headers: &HeaderMap) -> Self {
        match headers
            .get("x-ratelimit-resource")
            .and_then(|v| v.to_str().ok())
        {
            None => Self::Unknown,
            Some("") => Self::Unknown,
            Some(value) => match value {
                "core" => Self::Core,
                "search" => Self::Search,
                "graphql" => Self::Graphql,
                "integration_manifest" => Self::IntegrationManifest,
                "code_scanning_upload" => Self::CodeScanningUpload,
                "audit_log" => Self::AuditLog,
                "source_import" => Self::SourceImport,
                "dependency_snapshots" => Self::DependencySnapshots,
                "code_search" => Self::CodeSearch,
                "actions_runner_registration" => Self::ActionsRunnerRegistration,
                "scim" => Self::Scim,
                _ => Self::Other,
            },
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Search => "search",
            Self::Graphql => "graphql",
            Self::IntegrationManifest => "integration_manifest",
            Self::CodeScanningUpload => "code_scanning_upload",
            Self::AuditLog => "audit_log",
            Self::SourceImport => "source_import",
            Self::DependencySnapshots => "dependency_snapshots",
            Self::CodeSearch => "code_search",
            Self::ActionsRunnerRegistration => "actions_runner_registration",
            Self::Scim => "scim",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether a rate limit is the primary quota or a secondary abuse limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateLimitKind {
    Primary,
    Secondary,
}

impl RateLimitKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }
}

/// The rate-limit state of one response, parsed from its headers. A missing or
/// malformed header yields `None` for that field without suppressing its siblings.
#[derive(Debug, Clone)]
pub(crate) struct RateLimitSnapshot {
    pub(crate) limit: Option<i64>,
    pub(crate) remaining: Option<i64>,
    pub(crate) used: Option<i64>,
    pub(crate) reset_timestamp_seconds: Option<i64>,
    pub(crate) retry_after_seconds: Option<i64>,
    pub(crate) resource: RateLimitResource,
}

impl RateLimitSnapshot {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            limit: parse_i64_header(headers, "x-ratelimit-limit"),
            remaining: parse_i64_header(headers, "x-ratelimit-remaining"),
            used: parse_i64_header(headers, "x-ratelimit-used"),
            reset_timestamp_seconds: parse_i64_header(headers, "x-ratelimit-reset"),
            retry_after_seconds: parse_i64_header(headers, "retry-after"),
            resource: RateLimitResource::from_header(headers),
        }
    }

    /// Classify a response as a primary or secondary rate limit, if it is one.
    /// A `retry-after` header means GitHub said so, so it wins over `remaining`.
    pub(crate) fn classify(&self, status: StatusCode) -> Option<RateLimitKind> {
        match status.as_u16() {
            403 | 429 => {
                if self.retry_after_seconds.is_some() {
                    Some(RateLimitKind::Secondary)
                } else if self.remaining == Some(0) {
                    Some(RateLimitKind::Primary)
                } else if status.as_u16() == 429 {
                    Some(RateLimitKind::Secondary)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn parse_i64_header(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers.get(name)?.to_str().ok()?.trim().parse::<i64>().ok()
}

/// GitHub documents `retry-after` for secondary limits, but also reports them as
/// a 403 whose body names the limit. Ordinary permission errors share the status
/// and do not carry this text.
const SECONDARY_LIMIT_TEXT: &str = "secondary rate limit";

pub(crate) fn reports_secondary_limit(status: StatusCode, body: &str) -> bool {
    status == StatusCode::FORBIDDEN && body.to_ascii_lowercase().contains(SECONDARY_LIMIT_TEXT)
}

/// GitHub reports a GraphQL primary limit as a 200 whose `errors` array names
/// `RATE_LIMITED`. A GraphQL body is JSON, so a parse failure is not a limit.
pub(crate) fn reports_graphql_rate_limit(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("errors")
        .and_then(|errors| errors.as_array())
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                error.get("type").and_then(|kind| kind.as_str()) == Some("RATE_LIMITED")
            })
        })
}

/// The observable result of one GitHub response. The variants are the closed
/// outcome vocabulary the request counter labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Success,
    Redirection,
    ClientError,
    ServerError,
    RateLimited(RateLimitKind),
    TransportError,
    Other,
}

impl Outcome {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Redirection => "redirection",
            Self::ClientError => "client_error",
            Self::ServerError => "server_error",
            Self::RateLimited(_) => "rate_limited",
            Self::TransportError => "transport_error",
            Self::Other => "other",
        }
    }

    pub(crate) fn rate_limit_kind(&self) -> Option<RateLimitKind> {
        match self {
            Self::RateLimited(kind) => Some(*kind),
            _ => None,
        }
    }
}

/// The rate-limit state and outcome of one response. The snapshot is parsed
/// before the body is consumed; the outcome carries the classification.
#[derive(Debug, Clone)]
pub(crate) struct Observation {
    pub(crate) snapshot: RateLimitSnapshot,
    pub(crate) outcome: Outcome,
}

impl Observation {
    /// Classify a REST response, or any response whose body is not a GraphQL
    /// payload. `body` is `None` when the body could not be read.
    pub(crate) fn from_response(
        status: StatusCode,
        snapshot: RateLimitSnapshot,
        body: Option<&str>,
    ) -> Self {
        let outcome = classify_observed(status, &snapshot, body);
        Self { snapshot, outcome }
    }

    /// Classify a GraphQL response. A 200 whose body names `RATE_LIMITED` is a
    /// rate limit even though the status is success; every other response uses
    /// the shared status and header rules.
    pub(crate) fn from_graphql(
        status: StatusCode,
        snapshot: RateLimitSnapshot,
        body: Option<&str>,
    ) -> Self {
        let outcome = match body {
            Some(text) if status == StatusCode::OK && reports_graphql_rate_limit(text) => {
                Outcome::RateLimited(if snapshot.retry_after_seconds.is_some() {
                    RateLimitKind::Secondary
                } else {
                    RateLimitKind::Primary
                })
            }
            _ => classify_observed(status, &snapshot, body),
        };
        Self { snapshot, outcome }
    }
}

fn classify_observed(
    status: StatusCode,
    snapshot: &RateLimitSnapshot,
    body: Option<&str>,
) -> Outcome {
    if let Some(kind) = snapshot.classify(status) {
        return Outcome::RateLimited(kind);
    }
    if status == StatusCode::FORBIDDEN
        && body.is_some_and(|body| reports_secondary_limit(status, body))
    {
        return Outcome::RateLimited(RateLimitKind::Secondary);
    }
    match body {
        Some(_) if status.is_redirection() => Outcome::Redirection,
        Some(_) if status.is_success() => Outcome::Success,
        Some(_) if status.is_client_error() => Outcome::ClientError,
        Some(_) if status.is_server_error() => Outcome::ServerError,
        Some(_) => Outcome::Other,
        None if status.is_client_error() => Outcome::ClientError,
        None if status.is_server_error() => Outcome::ServerError,
        None => Outcome::Other,
    }
}

#[cfg(test)]
pub(crate) fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(
            http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            http::header::HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limit_headers() {
        let full = RateLimitSnapshot::from_headers(&header_map(&[
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "4999"),
            ("x-ratelimit-used", "1"),
            ("x-ratelimit-reset", "1760000000"),
            ("x-ratelimit-resource", "core"),
        ]));
        assert_eq!(full.limit, Some(5000), "limit");
        assert_eq!(full.remaining, Some(4999), "remaining");
        assert_eq!(full.used, Some(1), "used");
        assert_eq!(full.reset_timestamp_seconds, Some(1760000000), "reset");
        assert_eq!(full.retry_after_seconds, None, "retry-after");
        assert_eq!(full.resource, RateLimitResource::Core, "resource");

        let empty = RateLimitSnapshot::from_headers(&HeaderMap::new());
        assert_eq!(
            (
                empty.limit,
                empty.remaining,
                empty.used,
                empty.reset_timestamp_seconds,
                empty.retry_after_seconds
            ),
            (None, None, None, None, None),
            "absent headers leave every value unset"
        );
        assert_eq!(empty.resource, RateLimitResource::Unknown, "resource");

        let malformed = RateLimitSnapshot::from_headers(&header_map(&[
            ("x-ratelimit-reset", "not-a-number"),
            ("x-ratelimit-remaining", "42"),
        ]));
        assert_eq!(
            malformed.reset_timestamp_seconds, None,
            "malformed reset is dropped"
        );
        assert_eq!(
            malformed.remaining,
            Some(42),
            "a malformed header does not suppress its siblings"
        );

        let unmapped = RateLimitSnapshot::from_headers(&header_map(&[(
            "x-ratelimit-resource",
            "something_new",
        )]));
        assert_eq!(
            unmapped.resource,
            RateLimitResource::Other,
            "an unmapped resource collapses to other"
        );
        assert_eq!(unmapped.resource.as_str(), "other", "resource label");
    }

    #[test]
    fn classifies_rate_limited_responses() {
        let empty = RateLimitSnapshot::from_headers(&HeaderMap::new());
        let exhausted = RateLimitSnapshot {
            remaining: Some(0),
            ..empty.clone()
        };
        let retry_after = RateLimitSnapshot {
            retry_after_seconds: Some(60),
            ..empty.clone()
        };

        assert_eq!(
            retry_after.classify(StatusCode::FORBIDDEN),
            Some(RateLimitKind::Secondary),
            "403 with retry-after"
        );
        assert_eq!(
            retry_after.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Secondary),
            "429 with retry-after"
        );
        assert_eq!(
            exhausted.classify(StatusCode::FORBIDDEN),
            Some(RateLimitKind::Primary),
            "403 with no remaining quota"
        );
        assert_eq!(
            exhausted.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Primary),
            "429 with no remaining quota"
        );
        assert_eq!(
            empty.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Secondary),
            "429 without headers"
        );
        assert_eq!(empty.classify(StatusCode::FORBIDDEN), None, "403");
        assert_eq!(empty.classify(StatusCode::OK), None, "200");
        assert_eq!(
            empty.classify(StatusCode::INTERNAL_SERVER_ERROR),
            None,
            "500"
        );

        assert!(
            reports_secondary_limit(
                StatusCode::FORBIDDEN,
                r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#
            ),
            "403 whose body names a secondary limit"
        );
        assert!(
            !reports_secondary_limit(
                StatusCode::FORBIDDEN,
                r#"{"message":"Resource not accessible by integration"}"#
            ),
            "403 without a rate-limit message"
        );
        assert!(
            !reports_secondary_limit(StatusCode::OK, "secondary rate limit"),
            "2xx"
        );
    }

    #[test]
    fn classifies_graphql_rate_limited_bodies() {
        assert!(
            reports_graphql_rate_limit(r#"{"errors":[{"type":"RATE_LIMITED"}]}"#),
            "RATE_LIMITED error"
        );
        assert!(
            reports_graphql_rate_limit(
                r#"{"data":null,"errors":[{"type":"FORBIDDEN"},{"type":"RATE_LIMITED"}]}"#
            ),
            "RATE_LIMITED beside another error"
        );
        assert!(
            !reports_graphql_rate_limit(r#"{"errors":[{"type":"FORBIDDEN"}]}"#),
            "other GraphQL error"
        );
        assert!(
            !reports_graphql_rate_limit(r#"{"data":{"viewer":{"login":"octocat"}}}"#),
            "a successful GraphQL payload"
        );
        assert!(
            !reports_graphql_rate_limit("not json"),
            "a non-JSON body is not a limit"
        );
    }

    #[test]
    fn derives_outcomes_from_observations() {
        let full = RateLimitSnapshot::from_headers(&header_map(&[
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "4999"),
            ("x-ratelimit-used", "1"),
            ("x-ratelimit-reset", "1760000000"),
            ("x-ratelimit-resource", "core"),
        ]));
        let exhausted = RateLimitSnapshot {
            remaining: Some(0),
            ..full.clone()
        };
        let named = r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#;

        assert_eq!(
            Observation::from_response(
                StatusCode::FORBIDDEN,
                full.clone(),
                Some("permission denied")
            )
            .outcome,
            Outcome::ClientError,
            "plain permission 403"
        );
        assert_eq!(
            Observation::from_response(StatusCode::FOUND, full.clone(), Some("")).outcome,
            Outcome::Redirection,
            "readable redirection"
        );
        assert_eq!(
            Observation::from_response(StatusCode::BAD_REQUEST, full.clone(), Some("")).outcome,
            Outcome::ClientError,
            "readable client error"
        );
        assert_eq!(
            Observation::from_graphql(
                StatusCode::OK,
                full.clone(),
                Some(r#"{"data":{"viewer":{"login":"octocat"}}}"#)
            )
            .outcome,
            Outcome::Success,
            "GraphQL 200 without a rate limit"
        );
        assert_eq!(
            Observation::from_response(StatusCode::INTERNAL_SERVER_ERROR, full, Some("boom"))
                .outcome,
            Outcome::ServerError,
            "server error"
        );
        assert_eq!(
            Observation::from_response(StatusCode::FORBIDDEN, exhausted, Some(named)).outcome,
            Outcome::RateLimited(RateLimitKind::Primary),
            "header precedence keeps an exhausted window primary"
        );
    }
}
