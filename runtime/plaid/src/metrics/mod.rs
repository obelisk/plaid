use std::net::SocketAddr;
use std::string::FromUtf8Error;

use prometheus::core::Collector;
use prometheus::{Encoder, Registry, TextEncoder};
use serde::Deserialize;

#[derive(Debug)]
pub enum EncodeError {
    Prometheus(prometheus::Error),
    Utf8(FromUtf8Error),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prometheus(e) => write!(f, "{e}"),
            Self::Utf8(e) => write!(f, "{e}"),
        }
    }
}

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
    encoder: TextEncoder,
}

impl MetricsHandle {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            encoder: TextEncoder::new(),
        }
    }

    pub fn register(&self, collector: Box<dyn Collector>) -> Result<(), prometheus::Error> {
        self.registry.register(collector)
    }

    pub fn encode(&self) -> Result<String, EncodeError> {
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        self.encoder
            .encode(&metric_families, &mut buffer)
            .map_err(EncodeError::Prometheus)?;
        String::from_utf8(buffer).map_err(EncodeError::Utf8)
    }
}
