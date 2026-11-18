use std::time::Duration;

use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, Opts};

use crate::metrics::MetricsHandle;

/// Metrics for module-facing external API calls.
///
/// Every host function that reaches an external system (GitHub, Slack, Okta, AWS, ...) is
/// timed and counted here. The `api` label carries the API family and `function` the full
/// host function name, so a single family answers both "which external API is in the hot
/// path" and "which APIs carry breaking-change risk".
///
/// Calls that return `ApiError::TestMode` from an API implementation are recorded with
/// `error="test_mode"`. Calls rejected by a host macro's test-mode guard are not recorded.
pub struct ApiMetrics {
    calls: IntCounterVec,
    failures: IntCounterVec,
    duration: HistogramVec,
}

impl ApiMetrics {
    /// Register the API metrics with the global registry.
    ///
    /// Only called once at boot (from `Api::new` in `main`). Panics if the metric
    /// definitions are invalid or if the collectors are already registered, so it
    /// must not be called again after startup.
    pub fn register(handle: &MetricsHandle) -> Self {
        let calls = IntCounterVec::new(
            Opts::new(
                "plaid_api_calls_total",
                "Total number of external API calls made by modules",
            ),
            &["api", "function", "module"],
        )
        .expect("valid metric definition");

        let failures = IntCounterVec::new(
            Opts::new(
                "plaid_api_failures_total",
                "Total number of external API calls that returned an error",
            ),
            &["api", "function", "module", "error"],
        )
        .expect("valid metric definition");

        let duration = HistogramVec::new(
            HistogramOpts::new(
                "plaid_api_call_duration_seconds",
                "Wall-clock duration of an external API call",
            ),
            &["api", "function", "module"],
        )
        .expect("valid metric definition");

        handle
            .register(Box::new(calls.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(failures.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(duration.clone()))
            .expect("expected unique collector");

        Self {
            calls,
            failures,
            duration,
        }
    }

    /// Record a completed external API call.
    ///
    /// `error` is `None` on success and a coarse error class otherwise.
    pub fn record(
        &self,
        api: &str,
        function: &str,
        module: &str,
        duration: Duration,
        error: Option<&str>,
    ) {
        self.calls.with_label_values(&[api, function, module]).inc();
        self.duration
            .with_label_values(&[api, function, module])
            .observe(duration.as_secs_f64());

        if let Some(error) = error {
            self.failures
                .with_label_values(&[api, function, module, error])
                .inc();
        }
    }
}
