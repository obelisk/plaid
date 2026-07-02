use std::net::SocketAddr;

use prometheus::core::Collector;
use prometheus::{Encoder, Registry, TextEncoder};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct MetricsConfiguration {
    /// The address and port to listen on for Prometheus metrics scraping.
    pub listen_address: SocketAddr,
}

/// A generic handle to the Prometheus metrics registry.
///
/// Subsystems register their own collectors (pull) or counters/gauges (push)
/// with this handle. The metrics module itself has no domain knowledge.
pub struct MetricsHandle {
    registry: Registry,
}

impl MetricsHandle {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
        }
    }

    pub fn register(&self, collector: Box<dyn Collector>) -> Result<(), prometheus::Error> {
        self.registry.register(collector)
    }

    pub fn encode(&self) -> String {
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        TextEncoder::new()
            .encode(&metric_families, &mut buffer)
            .unwrap_or_default();
        String::from_utf8(buffer).unwrap_or_default()
    }
}
