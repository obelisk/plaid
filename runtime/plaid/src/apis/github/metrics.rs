use http::{Method, StatusCode};
use prometheus::{
    register_int_counter_vec_with_registry, register_int_gauge_vec_with_registry, IntCounterVec,
    IntGaugeVec,
};

use crate::metrics::MetricsHandle;

use super::rate_limit::{reports_secondary_limit, RateLimitKind, RateLimitSnapshot};

/// What the caller got out of a response, which decides whether a 2XX may be
/// recorded as `success`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Verdict {
    /// The body was read; the status describes the outcome.
    Read,
    /// The caller received an error from this response.
    Failed,
    /// A GraphQL 200 whose body carries a `RATE_LIMITED` error.
    GraphqlRateLimited,
    /// A 403 whose body names a secondary rate limit instead of sending
    /// `retry-after`.
    SecondaryRateLimited,
}

impl Verdict {
    /// Classify one observed response. `body` is `None` when the body could not
    /// be read. Header evidence keeps precedence, so an exhausted window stays
    /// primary even when the body also names the secondary limit.
    pub(crate) fn observed(
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        body: Option<&str>,
    ) -> Self {
        match body {
            Some(text)
                if snapshot.classify(status).is_none() && reports_secondary_limit(status, text) =>
            {
                Self::SecondaryRateLimited
            }
            Some(_) => Self::Read,
            None => Self::Failed,
        }
    }
}

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
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        verdict: Verdict,
    ) {
        let kind = match verdict {
            // GraphQL reports its primary rate limit as a 200 with a
            // RATE_LIMITED error, so the status is not enough to classify it.
            Verdict::GraphqlRateLimited => Some(if snapshot.retry_after_seconds.is_some() {
                RateLimitKind::Secondary
            } else {
                RateLimitKind::Primary
            }),
            Verdict::SecondaryRateLimited => Some(RateLimitKind::Secondary),
            Verdict::Read | Verdict::Failed => snapshot.classify(status),
        };
        let resource = snapshot.resource.as_str();

        self.requests_total
            .with_label_values(&[
                client,
                resource,
                method.as_str(),
                outcome(status, verdict, kind),
            ])
            .inc();

        self.update_gauges(client, snapshot);

        if let Some(kind) = kind {
            self.rate_limited_total
                .with_label_values(&[client, resource, kind.as_str()])
                .inc();
        }
    }

    /// Record a request that failed before a response arrived.
    pub(crate) fn record_transport_error(&self, client: &str, method: &Method) {
        self.requests_total
            .with_label_values(&[client, "unknown", method.as_str(), "transport_error"])
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

fn outcome(status: StatusCode, verdict: Verdict, kind: Option<RateLimitKind>) -> &'static str {
    match kind {
        Some(_) => "rate_limited",
        None if matches!(verdict, Verdict::Read)
            && (status.is_success() || status.is_redirection()) =>
        {
            "success"
        }
        None if status.is_client_error() => "client_error",
        None if status.is_server_error() => "server_error",
        None => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apis::github::rate_limit::header_map;
    use crate::metrics::MetricsHandle;

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

    struct Case {
        name: &'static str,
        method: Method,
        status: StatusCode,
        headers: &'static [(&'static str, &'static str)],
        verdict: Verdict,
        present: &'static [&'static str],
        absent: &'static [&'static str],
    }

    fn record_one(case: &Case) -> String {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle).unwrap();
        metrics.record(
            "test",
            &case.method,
            case.status,
            &RateLimitSnapshot::from_headers(&header_map(case.headers)),
            case.verdict,
        );
        handle.encode().unwrap()
    }

    #[test]
    fn records_each_observation() {
        let cases = [
            Case {
                name: "success with full headers",
                method: Method::GET,
                status: StatusCode::OK,
                headers: FULL_HEADERS,
                verdict: Verdict::Read,
                present: &[
                    "plaid_github_api_rate_limit_limit{client=\"test\",resource=\"core\"} 5000",
                    "plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999",
                    "plaid_github_api_rate_limit_used{client=\"test\",resource=\"core\"} 1",
                    "plaid_github_api_rate_limit_reset_timestamp_seconds{client=\"test\",resource=\"core\"} 1760000000",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"success\",resource=\"core\"} 1",
                ],
                absent: &["plaid_github_api_rate_limited_total"],
            },
            Case {
                name: "success without headers",
                method: Method::GET,
                status: StatusCode::OK,
                headers: &[],
                verdict: Verdict::Read,
                present: &[
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"success\",resource=\"unknown\"} 1",
                ],
                absent: &[
                    "plaid_github_api_rate_limit_limit{",
                    "plaid_github_api_rate_limit_remaining{",
                    "plaid_github_api_rate_limit_used{",
                    "plaid_github_api_rate_limit_reset_timestamp_seconds{",
                ],
            },
            Case {
                name: "403 with exhausted quota is a primary limit",
                method: Method::GET,
                status: StatusCode::FORBIDDEN,
                headers: EXHAUSTED_HEADERS,
                verdict: Verdict::Read,
                present: &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                absent: &[],
            },
            Case {
                name: "GraphQL 200 with a RATE_LIMITED body is a primary limit",
                method: Method::POST,
                status: StatusCode::OK,
                headers: GRAPHQL_EXHAUSTED_HEADERS,
                verdict: Verdict::GraphqlRateLimited,
                present: &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"graphql\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"rate_limited\",resource=\"graphql\"} 1",
                ],
                absent: &[],
            },
            Case {
                name: "403 whose body names a secondary limit",
                method: Method::GET,
                status: StatusCode::FORBIDDEN,
                headers: FULL_HEADERS,
                verdict: Verdict::SecondaryRateLimited,
                present: &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"secondary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                absent: &[],
            },
            Case {
                name: "failed body on a 200 is not a success",
                method: Method::POST,
                status: StatusCode::OK,
                headers: FULL_HEADERS,
                verdict: Verdict::Failed,
                present: &[
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"other\",resource=\"core\"} 1",
                    "plaid_github_api_rate_limit_remaining{client=\"test\",resource=\"core\"} 4999",
                ],
                absent: &["outcome=\"success\"", "plaid_github_api_rate_limited_total"],
            },
            Case {
                name: "failed body on an exhausted 403 is still a primary limit",
                method: Method::POST,
                status: StatusCode::FORBIDDEN,
                headers: EXHAUSTED_HEADERS,
                verdict: Verdict::Failed,
                present: &[
                    "plaid_github_api_rate_limited_total{client=\"test\",kind=\"primary\",resource=\"core\"} 1",
                    "plaid_github_api_requests_total{client=\"test\",method=\"POST\",outcome=\"rate_limited\",resource=\"core\"} 1",
                ],
                absent: &["outcome=\"success\""],
            },
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
    fn derives_verdicts_from_observed_responses() {
        let full = RateLimitSnapshot::from_headers(&header_map(FULL_HEADERS));
        let exhausted = RateLimitSnapshot::from_headers(&header_map(EXHAUSTED_HEADERS));
        let named = r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again."}"#;

        assert!(
            matches!(
                Verdict::observed(StatusCode::FORBIDDEN, &full, Some(named)),
                Verdict::SecondaryRateLimited
            ),
            "403 naming a secondary limit without a header signal"
        );
        assert!(
            matches!(
                Verdict::observed(StatusCode::FORBIDDEN, &full, Some("permission denied")),
                Verdict::Read
            ),
            "plain permission 403"
        );
        assert!(
            matches!(
                Verdict::observed(StatusCode::FORBIDDEN, &exhausted, Some(named)),
                Verdict::Read
            ),
            "header precedence keeps an exhausted window primary"
        );
        assert!(
            matches!(
                Verdict::observed(StatusCode::OK, &full, None),
                Verdict::Failed
            ),
            "unreadable body"
        );
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
