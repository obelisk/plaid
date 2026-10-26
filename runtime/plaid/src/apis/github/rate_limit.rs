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
}
