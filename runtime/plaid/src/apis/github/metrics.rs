use http::StatusCode;
use prometheus::{IntCounterVec, IntGaugeVec, Opts};

use crate::metrics::MetricsHandle;

use super::rate_limit::{RateLimitKind, RateLimitSnapshot};

/// Prometheus collectors for GitHub API quota state. Registered once per
/// runtime; gauges are only written from observed responses so an absent
/// series means "no GitHub traffic observed yet".
pub(crate) struct GithubMetrics {
    rate_limit_limit: IntGaugeVec,
    rate_limit_remaining: IntGaugeVec,
    rate_limit_used: IntGaugeVec,
    rate_limit_reset_timestamp_seconds: IntGaugeVec,
    requests_total: IntCounterVec,
    rate_limited_total: IntCounterVec,
}

impl GithubMetrics {
    pub(crate) fn register(handle: &MetricsHandle) -> Self {
        let rate_limit_limit = IntGaugeVec::new(
            Opts::new(
                "plaid_github_api_rate_limit_limit",
                "Rate-limit window ceiling reported by GitHub, in requests for REST and points for GraphQL",
            ),
            &["client", "resource"],
        )
        .expect("valid metric definition");

        let rate_limit_remaining = IntGaugeVec::new(
            Opts::new(
                "plaid_github_api_rate_limit_remaining",
                "Quota units remaining in the current rate-limit window",
            ),
            &["client", "resource"],
        )
        .expect("valid metric definition");

        let rate_limit_used = IntGaugeVec::new(
            Opts::new(
                "plaid_github_api_rate_limit_used",
                "Quota units used in the current rate-limit window",
            ),
            &["client", "resource"],
        )
        .expect("valid metric definition");

        let rate_limit_reset_timestamp_seconds = IntGaugeVec::new(
            Opts::new(
                "plaid_github_api_rate_limit_reset_timestamp_seconds",
                "Unix timestamp when the current rate-limit window resets",
            ),
            &["client", "resource"],
        )
        .expect("valid metric definition");

        let requests_total = IntCounterVec::new(
            Opts::new(
                "plaid_github_api_requests_total",
                "Total logical GitHub API requests. octocrab retry attempts are not counted separately",
            ),
            &["client", "resource", "method", "outcome"],
        )
        .expect("valid metric definition");

        let rate_limited_total = IntCounterVec::new(
            Opts::new(
                "plaid_github_api_rate_limited_total",
                "Total GitHub API responses classified as primary or secondary rate limits",
            ),
            &["client", "resource", "kind"],
        )
        .expect("valid metric definition");

        handle
            .register(Box::new(rate_limit_limit.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(rate_limit_remaining.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(rate_limit_used.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(rate_limit_reset_timestamp_seconds.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(requests_total.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(rate_limited_total.clone()))
            .expect("expected unique collector");

        Self {
            rate_limit_limit,
            rate_limit_remaining,
            rate_limit_used,
            rate_limit_reset_timestamp_seconds,
            requests_total,
            rate_limited_total,
        }
    }

    /// Record a completed response. Gauges are only written for headers GitHub
    /// actually returned, so an absent series means no value has been observed.
    pub(crate) fn record_headers(
        &self,
        client: &str,
        method: &str,
        status: StatusCode,
        headers: &http::HeaderMap,
    ) {
        let snapshot = RateLimitSnapshot::from_headers(headers);
        self.record(client, method, status, &snapshot, snapshot.classify(status));
    }

    /// GraphQL reports its primary rate limit as a 200 with a RATE_LIMITED
    /// error, so the status is not enough to classify it.
    pub(crate) fn record_graphql(
        &self,
        client: &str,
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        rate_limited: bool,
    ) {
        let kind = if rate_limited {
            Some(if snapshot.retry_after_seconds.is_some() {
                RateLimitKind::Secondary
            } else {
                RateLimitKind::Primary
            })
        } else {
            snapshot.classify(status)
        };
        self.record(client, "graphql", status, snapshot, kind);
    }

    fn record(
        &self,
        client: &str,
        method: &str,
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        kind: Option<RateLimitKind>,
    ) {
        let resource = snapshot.resource.as_str();

        self.requests_total
            .with_label_values(&[client, resource, method, outcome(status, kind)])
            .inc();

        self.update_gauges(client, snapshot);

        if let Some(kind) = kind {
            self.rate_limited_total
                .with_label_values(&[client, resource, kind.as_str()])
                .inc();
        }
    }

    /// Records a response whose headers arrived but whose body could not be
    /// read. The caller received an error, so a success-class status never
    /// records as `success`.
    pub(crate) fn record_unreadable_body(
        &self,
        client: &str,
        method: &str,
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
    ) {
        let kind = snapshot.classify(status);
        let outcome = match kind {
            Some(_) => "rate_limited",
            None if status.is_client_error() => "client_error",
            None if status.is_server_error() => "server_error",
            None => "other",
        };

        self.requests_total
            .with_label_values(&[client, snapshot.resource.as_str(), method, outcome])
            .inc();
        self.update_gauges(client, snapshot);

        if let Some(kind) = kind {
            self.rate_limited_total
                .with_label_values(&[
                    client,
                    snapshot.resource.as_str(),
                    kind.as_str(),
                ])
                .inc();
        }
    }

    fn update_gauges(&self, client: &str, snapshot: &RateLimitSnapshot) {
        let resource = snapshot.resource.as_str();

        // Only write a gauge when the response carried the header. Zero is a
        // meaningful exhaustion signal, so there is no zero-initialization.
        if let Some(limit) = snapshot.limit {
            self.rate_limit_limit
                .with_label_values(&[client, resource])
                .set(limit);
        }
        if let Some(remaining) = snapshot.remaining {
            self.rate_limit_remaining
                .with_label_values(&[client, resource])
                .set(remaining);
        }
        if let Some(used) = snapshot.used {
            self.rate_limit_used
                .with_label_values(&[client, resource])
                .set(used);
        }
        if let Some(reset) = snapshot.reset_timestamp_seconds {
            self.rate_limit_reset_timestamp_seconds
                .with_label_values(&[client, resource])
                .set(reset);
        }
    }

    /// Record a request that failed before a response arrived.
    pub(crate) fn record_transport_error(&self, client: &str, method: &str) {
        self.requests_total
            .with_label_values(&[client, "unknown", method, "transport_error"])
            .inc();
    }
}

fn outcome(status: StatusCode, kind: Option<RateLimitKind>) -> &'static str {
    match kind {
        Some(_) => "rate_limited",
        None if status.is_success() || status.is_redirection() => "success",
        None if status.is_client_error() => "client_error",
        None if status.is_server_error() => "server_error",
        None => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsHandle;

    fn full_rate_limit_headers() -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        for (name, value) in [
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "4999"),
            ("x-ratelimit-used", "1"),
            ("x-ratelimit-reset", "1760000000"),
            ("x-ratelimit-resource", "core"),
        ] {
            headers.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                http::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    fn metrics() -> (MetricsHandle, GithubMetrics) {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle);
        (handle, metrics)
    }

    #[test]
    fn full_headers_set_gauges_and_success_counter() {
        let (handle, metrics) = metrics();
        metrics.record_headers(
            "test",
            "get",
            StatusCode::OK,
            &full_rate_limit_headers(),
        );

        let encoded = handle.encode().unwrap();
        assert!(encoded
            .contains("plaid_github_api_rate_limit_limit{client=\"test\",resource=\"core\"} 5000"));
        assert!(encoded.contains(
            "plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999"
        ));
        assert!(encoded
            .contains("plaid_github_api_rate_limit_used{client=\"test\",resource=\"core\"} 1"));
        assert!(encoded.contains("plaid_github_api_rate_limit_reset_timestamp_seconds{client=\"test\",resource=\"core\"} 1760000000"));
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"get\",outcome=\"success\",resource=\"core\"} 1"));
        assert!(!encoded.contains("plaid_github_api_rate_limited_total"));
    }

    #[test]
    fn missing_headers_only_increment_the_request_counter() {
        let (handle, metrics) = metrics();
        metrics.record_headers(
            "test",
            "get",
            StatusCode::OK,
            &http::HeaderMap::new(),
        );

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"get\",outcome=\"success\",resource=\"unknown\"} 1"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_limit{"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_remaining{"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_used{"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_reset_timestamp_seconds{"));
    }

    #[test]
    fn exhausted_403_counts_as_primary_rate_limit() {
        let (handle, metrics) = metrics();
        let mut headers = full_rate_limit_headers();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        metrics.record_headers("test", "get", StatusCode::FORBIDDEN, &headers);

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1"));
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"get\",outcome=\"rate_limited\",resource=\"core\"} 1"));
    }

    #[test]
    fn graphql_body_rate_limit_counts_as_primary() {
        let (handle, metrics) = metrics();
        let mut headers = http::HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5000".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        headers.insert("x-ratelimit-resource", "graphql".parse().unwrap());

        let snapshot = RateLimitSnapshot::from_headers(&headers);
        metrics.record_graphql("test", StatusCode::OK, &snapshot, true);

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"graphql\"} 1"));
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"graphql\",outcome=\"rate_limited\",resource=\"graphql\"} 1"));
    }

    #[test]
    fn unreadable_body_never_records_success() {
        let (handle, metrics) = metrics();
        let headers = full_rate_limit_headers();
        let snapshot = RateLimitSnapshot::from_headers(&headers);

        metrics.record_unreadable_body("test", "graphql", StatusCode::OK, &snapshot);

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"graphql\",outcome=\"other\",resource=\"core\"} 1"));
        assert!(!encoded.contains("outcome=\"success\""));
        assert!(encoded
            .contains("plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999"));
        assert!(!encoded.contains("plaid_github_api_rate_limited_total"));

        let mut exhausted = full_rate_limit_headers();
        exhausted.insert(
            http::header::HeaderName::from_bytes(b"x-ratelimit-remaining").unwrap(),
            http::header::HeaderValue::from_str("0").unwrap(),
        );
        metrics.record_unreadable_body(
            "test",
            "graphql",
            StatusCode::FORBIDDEN,
            &RateLimitSnapshot::from_headers(&exhausted),
        );
        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1"));
    }

    #[test]
    fn transport_error_increments_only_the_transport_series() {
        let (handle, metrics) = metrics();
        metrics.record_transport_error("test", "get");

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"get\",outcome=\"transport_error\",resource=\"unknown\"} 1"));
        assert!(!encoded.contains("outcome=\"success\""));
        assert!(!encoded.contains("plaid_github_api_rate_limited_total"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_limit{"));
    }
}
