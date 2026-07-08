use std::collections::HashMap;

use crossbeam_channel::Sender;
use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts};

use crate::metrics::MetricsHandle;

use super::thread_pools::ExecutionThreadPools;
use super::Message;

/// Histograms for per-module execution stats, updated after each successful run.
pub struct ModuleExecutionMetrics {
    computation_percentage: HistogramVec,
    execution_duration_seconds: HistogramVec,
    module_failures: IntCounterVec,
}

impl ModuleExecutionMetrics {
    pub fn register(handle: &MetricsHandle) -> Self {
        let computation_percentage = HistogramVec::new(
            HistogramOpts::new(
                "plaid_module_computation_percentage_used",
                "Percentage of a module's computation budget consumed per execution",
            )
            .buckets(vec![10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 100.0]),
            &["module"],
        )
        .expect("valid metric definition");

        let execution_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "plaid_module_execution_duration_seconds",
                "Wall-clock duration of a successful module execution",
            ),
            &["module"],
        )
        .expect("valid metric definition");

        let module_failures = IntCounterVec::new(Opts::new("name", "help"), &["module"])
            .expect("valid metric definition");

        handle
            .register(Box::new(computation_percentage.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(execution_duration_seconds.clone()))
            .expect("expected unique collector");
        handle
            .register(Box::new(module_failures.clone()))
            .expect("expected unique collector");

        Self {
            computation_percentage,
            execution_duration_seconds,
            module_failures,
        }
    }

    pub fn record_successful_execution(
        &self,
        module: &str,
        computation_used_percentage: f64,
        duration: std::time::Duration,
    ) {
        self.computation_percentage
            .with_label_values(&[module])
            .observe(computation_used_percentage);
        self.execution_duration_seconds
            .with_label_values(&[module])
            .observe(duration.as_secs_f64());
    }

    pub fn record_module_failure(&self, module: &str) {
        self.module_failures.with_label_values(&[module]).inc();
    }
}

/// Reports depth and percentage capacity of each execution queue. Values are
/// read from the live channel senders at scrape time, so there are no writers.
pub struct QueueMetrics {
    /// One sender per queue: "general" + each dedicated log type.
    senders: HashMap<String, Sender<Message>>,
    depth: IntGaugeVec,
    capacity_percentage: GaugeVec,
}

impl QueueMetrics {
    pub fn register(handle: &MetricsHandle, pools: &ExecutionThreadPools) {
        handle
            .register(Box::new(Self::new(pools)))
            .expect("expected unique collector");
    }

    fn new(pools: &ExecutionThreadPools) -> Self {
        let mut senders = HashMap::new();
        senders.insert("general".to_string(), pools.general_pool.sender.clone());
        for (log_type, tp) in &pools.dedicated_pools {
            senders.insert(log_type.clone(), tp.sender.clone());
        }

        let depth = IntGaugeVec::new(
            Opts::new(
                "plaid_execution_queue_depth",
                "Number of messages currently queued for execution",
            ),
            &["queue"],
        )
        .expect("valid metric definition");

        let capacity_percentage = GaugeVec::new(
            Opts::new(
                "plaid_execution_queue_capacity_percentage",
                "Percentage of the execution queue's capacity currently in use",
            ),
            &["queue"],
        )
        .expect("valid metric definition");

        Self {
            senders,
            depth,
            capacity_percentage,
        }
    }
}

impl Collector for QueueMetrics {
    fn desc(&self) -> Vec<&Desc> {
        let mut descs = self.depth.desc();
        descs.extend(self.capacity_percentage.desc());
        descs
    }

    fn collect(&self) -> Vec<MetricFamily> {
        for (queue, sender) in &self.senders {
            let depth = sender.len();
            let capacity = sender.capacity().unwrap_or(0);
            let pct = if capacity == 0 {
                0.0
            } else {
                (depth as f64 / capacity as f64) * 100.0
            };
            self.depth.with_label_values(&[queue]).set(depth as i64);
            self.capacity_percentage
                .with_label_values(&[queue])
                .set(pct);
        }

        let mut families = self.depth.collect();
        families.extend(self.capacity_percentage.collect());
        families
    }
}
