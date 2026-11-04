use http::Method;
use prometheus::{
    register_int_counter_vec_with_registry, register_int_gauge_vec_with_registry, IntCounterVec,
    IntGaugeVec,
};

use crate::metrics::{LabelSet, MetricsHandle};

use super::response::{Outcome, RateLimitKind, RateLimitResource, RateLimitSnapshot};

struct GaugeLabels<'a> {
    client: &'a str,
    resource: RateLimitResource,
}

impl GaugeLabels<'_> {
    fn values(&self) -> [&str; 2] {
        [self.client, self.resource.as_str()]
    }
}

impl LabelSet for GaugeLabels<'_> {
    const NAMES: &'static [&'static str] = &["client", "resource"];
}

struct RequestLabels<'a> {
    client: &'a str,
    resource: RateLimitResource,
    method: &'a Method,
    outcome: Outcome,
}

impl RequestLabels<'_> {
    fn values(&self) -> [&str; 4] {
        [
            self.client,
            self.resource.as_str(),
            self.method.as_str(),
            self.outcome.as_str(),
        ]
    }
}

impl LabelSet for RequestLabels<'_> {
    const NAMES: &'static [&'static str] = &["client", "resource", "method", "outcome"];
}

struct KindLabels<'a> {
    client: &'a str,
    resource: RateLimitResource,
    kind: RateLimitKind,
}

impl KindLabels<'_> {
    fn values(&self) -> [&str; 3] {
        [self.client, self.resource.as_str(), self.kind.as_str()]
    }
}

impl LabelSet for KindLabels<'_> {
    const NAMES: &'static [&'static str] = &["client", "resource", "kind"];
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
            GaugeLabels::NAMES,
            registry,
        )?;

        let rate_limit_remaining = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_remaining",
            "Quota units remaining in the current rate-limit window",
            GaugeLabels::NAMES,
            registry,
        )?;

        let rate_limit_used = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_used",
            "Quota units used in the current rate-limit window",
            GaugeLabels::NAMES,
            registry,
        )?;

        let rate_limit_reset_timestamp_seconds = register_int_gauge_vec_with_registry!(
            "plaid_github_api_rate_limit_reset_timestamp_seconds",
            "Unix timestamp when the current rate-limit window resets",
            GaugeLabels::NAMES,
            registry,
        )?;

        let requests_total = register_int_counter_vec_with_registry!(
            "plaid_github_api_requests_total",
            "Total logical GitHub API requests. octocrab retry attempts are not counted separately",
            RequestLabels::NAMES,
            registry,
        )?;

        let rate_limited_total = register_int_counter_vec_with_registry!(
            "plaid_github_api_rate_limited_total",
            "Total GitHub API responses classified as primary or secondary rate limits",
            KindLabels::NAMES,
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
        let labels = RequestLabels {
            client,
            resource: snapshot.resource,
            method,
            outcome,
        };
        self.requests_total
            .with_label_values(&labels.values())
            .inc();

        self.update_gauges(client, snapshot);

        if let Some(kind) = outcome.rate_limit_kind() {
            let labels = KindLabels {
                client,
                resource: snapshot.resource,
                kind,
            };
            self.rate_limited_total
                .with_label_values(&labels.values())
                .inc();
        }
    }

    /// Record a request that failed before a response arrived.
    pub(crate) fn record_transport_error(&self, client: &str, method: &Method) {
        let labels = RequestLabels {
            client,
            resource: RateLimitResource::Unknown,
            method,
            outcome: Outcome::TransportError,
        };
        self.requests_total
            .with_label_values(&labels.values())
            .inc();
    }

    fn update_gauges(&self, client: &str, snapshot: &RateLimitSnapshot) {
        let labels = GaugeLabels {
            client,
            resource: snapshot.resource,
        };
        let values = labels.values();

        if let Some(limit) = snapshot.limit {
            self.rate_limit_limit.with_label_values(&values).set(limit);
        }
        if let Some(remaining) = snapshot.remaining {
            self.rate_limit_remaining
                .with_label_values(&values)
                .set(remaining);
        }
        if let Some(used) = snapshot.used {
            self.rate_limit_used.with_label_values(&values).set(used);
        }
        if let Some(reset) = snapshot.reset_timestamp_seconds {
            self.rate_limit_reset_timestamp_seconds
                .with_label_values(&values)
                .set(reset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apis::github::response::{classify, header_map, ApiKind, Outcome, RateLimitKind};
    use crate::metrics::{label_pairs, MetricsHandle};
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

    fn record_one(case: &Case) -> Result<String, Box<dyn std::error::Error>> {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle)?;
        metrics.record("test", &case.method, &case.snapshot, case.outcome);
        Ok(handle.encode()?)
    }

    #[test]
    fn records_each_outcome() -> Result<(), Box<dyn std::error::Error>> {
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
            let encoded = record_one(&case)?;
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

        Ok(())
    }

    #[test]
    fn records_transport_errors() -> Result<(), Box<dyn std::error::Error>> {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle)?;
        metrics.record_transport_error("test", &Method::GET);

        let encoded = handle.encode()?;
        assert!(encoded.contains("plaid_github_api_requests_total{client=\"test\",method=\"GET\",outcome=\"transport_error\",resource=\"unknown\"} 1"));
        assert!(!encoded.contains("outcome=\"success\""));
        assert!(!encoded.contains("plaid_github_api_rate_limited_total"));
        assert!(!encoded.contains("plaid_github_api_rate_limit_limit{"));

        Ok(())
    }

    #[test]
    fn every_series_is_registered() -> Result<(), Box<dyn std::error::Error>> {
        let handle = MetricsHandle::new();
        let metrics = GithubMetrics::register(&handle)?;
        metrics.record(
            "test",
            &Method::GET,
            &snapshot(FULL_HEADERS),
            Outcome::RateLimited(RateLimitKind::Primary),
        );

        let encoded = handle.encode()?;
        let expected = [
            (
                "gauge",
                "plaid_github_api_rate_limit_limit",
                "Rate-limit window ceiling reported by GitHub, in requests for REST and points for GraphQL",
            ),
            (
                "gauge",
                "plaid_github_api_rate_limit_remaining",
                "Quota units remaining in the current rate-limit window",
            ),
            (
                "gauge",
                "plaid_github_api_rate_limit_used",
                "Quota units used in the current rate-limit window",
            ),
            (
                "gauge",
                "plaid_github_api_rate_limit_reset_timestamp_seconds",
                "Unix timestamp when the current rate-limit window resets",
            ),
            (
                "counter",
                "plaid_github_api_requests_total",
                "Total logical GitHub API requests. octocrab retry attempts are not counted separately",
            ),
            (
                "counter",
                "plaid_github_api_rate_limited_total",
                "Total GitHub API responses classified as primary or secondary rate limits",
            ),
        ];

        for (kind, name, help) in expected {
            assert!(
                encoded.contains(&format!("# TYPE {name} {kind}")),
                "missing # TYPE for {name}"
            );
            assert!(
                encoded.contains(&format!("# HELP {name} {help}")),
                "missing # HELP for {name}"
            );
        }

        Ok(())
    }

    #[test]
    fn every_label_set_pairs_its_names_with_values() {
        let gauge = GaugeLabels {
            client: "client-a",
            resource: RateLimitResource::Core,
        };
        assert_eq!(
            label_pairs(GaugeLabels::NAMES, &gauge.values()),
            [("client", "client-a"), ("resource", "core")]
        );

        let request = RequestLabels {
            client: "client-b",
            resource: RateLimitResource::Search,
            method: &Method::POST,
            outcome: Outcome::RateLimited(RateLimitKind::Primary),
        };
        assert_eq!(
            label_pairs(RequestLabels::NAMES, &request.values()),
            [
                ("client", "client-b"),
                ("resource", "search"),
                ("method", "POST"),
                ("outcome", "rate_limited"),
            ]
        );

        let kind = KindLabels {
            client: "client-c",
            resource: RateLimitResource::Graphql,
            kind: RateLimitKind::Secondary,
        };
        assert_eq!(
            label_pairs(KindLabels::NAMES, &kind.values()),
            [
                ("client", "client-c"),
                ("resource", "graphql"),
                ("kind", "secondary"),
            ]
        );
    }
}
