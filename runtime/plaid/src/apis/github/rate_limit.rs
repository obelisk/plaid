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

#[cfg(test)]
mod tests {
    use super::*;

    fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                http::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn parses_full_core_headers() {
        let headers = header_map(&[
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "4999"),
            ("x-ratelimit-used", "1"),
            ("x-ratelimit-reset", "1760000000"),
            ("x-ratelimit-resource", "core"),
        ]);
        let snapshot = RateLimitSnapshot::from_headers(&headers);
        assert_eq!(snapshot.limit, Some(5000));
        assert_eq!(snapshot.remaining, Some(4999));
        assert_eq!(snapshot.used, Some(1));
        assert_eq!(snapshot.reset_timestamp_seconds, Some(1760000000));
        assert_eq!(snapshot.retry_after_seconds, None);
        assert_eq!(snapshot.resource, RateLimitResource::Core);
    }

    #[test]
    fn empty_headers_yield_unknown_resource() {
        let snapshot = RateLimitSnapshot::from_headers(&HeaderMap::new());
        assert_eq!(snapshot.limit, None);
        assert_eq!(snapshot.remaining, None);
        assert_eq!(snapshot.used, None);
        assert_eq!(snapshot.reset_timestamp_seconds, None);
        assert_eq!(snapshot.retry_after_seconds, None);
        assert_eq!(snapshot.resource, RateLimitResource::Unknown);
    }

    #[test]
    fn malformed_reset_does_not_suppress_remaining() {
        let headers = header_map(&[
            ("x-ratelimit-reset", "not-a-number"),
            ("x-ratelimit-remaining", "42"),
        ]);
        let snapshot = RateLimitSnapshot::from_headers(&headers);
        assert_eq!(snapshot.reset_timestamp_seconds, None);
        assert_eq!(snapshot.remaining, Some(42));
    }

    #[test]
    fn unknown_resource_string_collapses_to_other() {
        let headers = header_map(&[("x-ratelimit-resource", "something_new")]);
        let snapshot = RateLimitSnapshot::from_headers(&headers);
        assert_eq!(snapshot.resource, RateLimitResource::Other);
        assert_eq!(snapshot.resource.as_str(), "other");
    }

    #[test]
    fn classifies_the_full_matrix() {
        let no_headers = RateLimitSnapshot::from_headers(&HeaderMap::new());
        let exhausted = RateLimitSnapshot {
            remaining: Some(0),
            ..RateLimitSnapshot::from_headers(&HeaderMap::new())
        };
        let retry_after = RateLimitSnapshot {
            retry_after_seconds: Some(60),
            ..RateLimitSnapshot::from_headers(&HeaderMap::new())
        };
        assert_eq!(
            retry_after.classify(StatusCode::FORBIDDEN),
            Some(RateLimitKind::Secondary)
        );
        assert_eq!(
            retry_after.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Secondary)
        );
        assert_eq!(
            exhausted.classify(StatusCode::FORBIDDEN),
            Some(RateLimitKind::Primary)
        );
        assert_eq!(
            exhausted.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Primary)
        );
        assert_eq!(
            no_headers.classify(StatusCode::TOO_MANY_REQUESTS),
            Some(RateLimitKind::Secondary)
        );
        assert_eq!(no_headers.classify(StatusCode::FORBIDDEN), None);
        assert_eq!(no_headers.classify(StatusCode::OK), None);
        assert_eq!(no_headers.classify(StatusCode::INTERNAL_SERVER_ERROR), None);
    }
}
