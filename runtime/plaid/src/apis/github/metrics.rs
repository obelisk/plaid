use http::Method;
use prometheus::{
    register_int_counter_vec_with_registry, register_int_gauge_vec_with_registry, IntCounterVec,
    IntGaugeVec,
};

use crate::metrics::MetricsHandle;

use super::response::{Outcome, RateLimitSnapshot};

/// Prometheus collectors for GitHub API quota state. Registered once per
/// runtime; gauges are only written from observed responses so an absent
/// series means "no GitHub traffic observed yet".
pub struct GithubMetrics {
    rate_limit_limit: IntGaugeVec,
    rate_limit_remaining: IntGaugeVec,
    rate_limit_used: IntGaugeVec,
    rate_limit_reset_timestamp_seconds: IntGaugeVec,
    requests_total: IntCounterVec,
    rate_limited_total: IntCounterVec,
}

impl GithubMetrics {
    pub fn register(handle: &MetricsHandle) -> Result<Self, prometheus::Error> {
        let registry = handle.registry();

        let rate_limit_limit = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_limit",
            "Rate-limit window ceiling reported by GitHub, in requests for REST and points for GraphQL",
            &["client", "resource"],
            registry,
        )?;

        let rate_limit_remaining = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_remaining",
            "Quota units remaining in the current rate-limit window",
            &["client", "resource"],
            registry,
        )?;

        let rate_limit_used = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_used",
            "Quota units used in the current rate-limit window",
            &["client", "resource"],
            registry,
        )?;

        let rate_limit_reset_timestamp_seconds = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_reset_timestamp_seconds",
            "Unix timestamp when the current rate-limit window resets",
            &["client", "resource"],
            registry,
        )?;

        let requests_total = register_int_counter_vec_with_registry!(
            "plaid_github_api_requests_total",
            "Total logical GitHub API requests. octocrab retry attempts are not counted separately",
            &["client", "resource", "method", "outcome"],
            registry,
        )?;

        let rate_limited_total = register_int_counter_vec_with_registry!(
            "plaid_github_api_rate_limited_total",
            "Total GitHub API responses classified as primary or secondary rate limits",
            &["client", "resource", "kind"],
            registry,
        )?;

        Ok(Self {
            rate_limit_limit,
            rate_limit_remaining,
            rate_limit_used,
            rate_limit_reset_timestamp_seconds,
            requests_total,
            rate_limited_total,
        })
    }

    /// Record one observed response: the request counter, whatever quota state
    /// GitHub reported, and the rate-limited counter when the response is one.
    pub(crate) fn record(
        &self,
        client: &str,
        method: &Method,
        snapshot: &RateLimitSnapshot,
        outcome: Outcome,
    ) {
        let resource = snapshot.resource.as_str();

        self.requests_total
            .with_label_values(&[client, resource, method.as_str(), outcome.as_str()])
            .inc();

        self.update_gauges(client, snapshot);

        if let Some(kind) = outcome.rate_limit_kind() {
            self.rate_limited_total
                .with_label_values(&[client, resource, kind.as_str()])
                .inc();
        }
    }

    /// Record a request that failed before a response arrived.
    pub(crate) fn record_transport_error(&self, client: &str, method: &Method) {
        self.requests_total
            .with_label_values(&[
                client,
                "unknown",
                method.as_str(),
                Outcome::TransportError.as_str(),
            ])
            .inc();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apis::github::response::{classify, header_map, ApiKind, Outcome};
    use crate::metrics::MetricsHandle;
    use http::StatusCode;

    const FULL_HEADERS: &[(&str, &str)] = &[
        ("x-ratelimit-limit", "5000"),
        ("x-ratelimit-remaining", "4999"),
        ("x-ratelimit-used", "1"),
        ("x-ratelimit-reset", "1760000000"),
        ("x-ratelimit-resource", "core"),
    ];
    const EXHAUSTED_HEADERS: &[(&str, &str)] = &[
        ("x-ratelimit-limit", "5000"),
        ("x-ratelimit-remaining", "0"),
        ("x-ratelimit-used", "5000"),
        ("x-ratelimit-reset", "1760000000"),
        ("x-ratelimit-resource", "core"),
    ];
    const GRAPHQL_EXHAUSTED_HEADERS: &[(&str, &str)] = &[
        ("x-ratelimit-limit", "5000"),
        ("x-ratelimit-remaining", "0"),
        ("x-ratelimit-resource", "graphql"),
    ];
    const NAMED_SECONDARY: &str = r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#;

    struct Case {
        name: &'static str,
        method: Method,
        snapshot: RateLimitSnapshot,
        outcome: Outcome,
        present: &'static [&'static str],
        absent: &'static [&'static str],
    }

    fn snapshot(headers: &[(&str, &str)]) -> RateLimitSnapshot {
        RateLimitSnapshot::from_headers(&header_map(headers))
    }

    #[allow(clippy::too_many_arguments)]
    fn case(
        name: &'static str,
        method: Method,
        api: ApiKind,
        status: StatusCode,
        snapshot: RateLimitSnapshot,
        body: Option<&str>,
        present: &'static [&'static str],
        absent: &'static [&'static str],
    ) -> Case {
        let outcome = classify(api, status, &snapshot, body);
        Case {
            name,
            method,
            snapshot,
            outcome,
            present,
            absent,
        }
    }

    fn record_one(case: &Case) -> String {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle).unwrap();
        metrics.record("test", &case.method, &case.snapshot, case.outcome);
        handle.encode().unwrap()
    }

    #[test]
    fn records_each_outcome() {
        let cases = [
            case(
                "success with full headers",
                Method::GET,
                ApiKind::Rest,
                StatusCode::OK,
                snapshot(FULL_HEADERS),
                Some(""),
                &[
                    "plaid_github_api_rate_limit_limit{client=\"test\",resource=\"core\"} 5000",
                    "plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999",
                    "plaid_github_api_rate_limit_used{client=\"test\",resource=\"core\"} 1",
                    "plaid_github_api_rate_limit_reset_timestamp_seconds{client=\"test\",resource=\"core\"} 1760000000",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"success\",resource=\"core\"} 1",
                ],
                &["plaid_github_api_rate_limited_total"],
            ),
            case(
                "success without headers",
                Method::GET,
                ApiKind::Rest,
                StatusCode::OK,
                snapshot(&[]),
                Some(""),
                &[
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"success\",resource=\"unknown\"} 1",
                ],
                &[
                    "plaid_github_api_rate_limit_limit{",
                    "plaid_github_api_rate_limit_remaining{",
                    "plaid_github_api_rate_limit_used{",
                    "plaid_github_api_rate_limit_reset_timestamp_seconds{",
                ],
            ),
            case(
                "403 with exhausted quota is a primary limit",
                Method::GET,
                ApiKind::Rest,
                StatusCode::FORBIDDEN,
                snapshot(EXHAUSTED_HEADERS),
                Some(""),
                &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                &[],
            ),
            case(
                "GraphQL 200 with a RATE_LIMITED body is a primary limit",
                Method::POST,
                ApiKind::GraphQL,
                StatusCode::OK,
                snapshot(GRAPHQL_EXHAUSTED_HEADERS),
                Some(r#"{"errors":[{"type":"RATE_LIMITED"}]}"#),
                &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"graphql\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"rate_limited\",resource=\"graphql\"} 1",
                ],
                &[],
            ),
            case(
                "403 whose body names a secondary limit",
                Method::GET,
                ApiKind::Rest,
                StatusCode::FORBIDDEN,
                snapshot(FULL_HEADERS),
                Some(NAMED_SECONDARY),
                &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"secondary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                &[],
            ),
            case(
                "failed body on a 200 is not a success",
                Method::POST,
                ApiKind::Rest,
                StatusCode::OK,
                snapshot(FULL_HEADERS),
                None,
                &[
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"other\",resource=\"core\"} 1",
                    "plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999",
                ],
                &["outcome=\"success\"", "plaid_github_api_rate_limited_total"],
            ),
            case(
                "failed body on an exhausted 403 is still a primary limit",
                Method::POST,
                ApiKind::Rest,
                StatusCode::FORBIDDEN,
                snapshot(EXHAUSTED_HEADERS),
                None,
                &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                &["outcome=\"success\""],
            ),
        ];

        for case in cases {
            let encoded = record_one(&case);
            for fragment in case.present {
                assert!(
                    encoded.contains(fragment),
                    "{}: expected {fragment}",
                    case.name
                );
            }
            for fragment in case.absent {
                assert!(
                    !encoded.contains(fragment),
                    "{}: did not expect {fragment}",
                    case.name
                );
            }
        }
    }

    #[test]
    fn records_transport_errors() {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle).unwrap();
        metrics.record_transport_error("test", &Method::GET);

        let encoded = handle.encode().unwrap();
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"transport_error\",resource=\"unknown\"} 1"));
        assert!(!encoded.contains("outcome=\"success\""));
        assert!(!encoded.contains("plaid_github_api_rate_limited_total"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_limit{"));
    }
}
