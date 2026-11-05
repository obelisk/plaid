use std::collections::HashMap;

use crossbeam_channel::Sender;
use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts};

use crate::metrics::{LabelSet, MetricsHandle};

use super::thread_pools::ExecutionThreadPools;
use super::Message;

struct ModuleLabels<'a> {
    module: &'a str,
}

impl ModuleLabels<'_> {
    fn values(&self) -> [&str; 1] {
        [self.module]
    }
}

impl LabelSet for ModuleLabels<'_> {
    const NAMES: &'static [&'static str] = &["module"];
}

struct QueueLabels<'a> {
    queue: &'a str,
}

impl QueueLabels<'_> {
    fn values(&self) -> [&str; 1] {
        [self.queue]
    }
}

impl LabelSet for QueueLabels<'_> {
    const NAMES: &'static [&'static str] = &["queue"];
}

/// Metrics for per-module execution stats
pub struct ModuleExecutionMetrics {
    computation_percentage: HistogramVec,
    execution_duration_seconds: HistogramVec,
    module_failures: IntCounterVec,
}

impl ModuleExecutionMetrics {
    pub fn register(handle: &MetricsHandle) -> Result<Self, prometheus::Error> {
        let computation_percentage = HistogramVec::new(
            HistogramOpts::new(
                "plaid_module_computation_percentage_used",
                "Percentage of a module's computation budget consumed per execution",
            )
            .buckets(vec![10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 100.0]),
            ModuleLabels::NAMES,
        )?;

        let execution_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "plaid_module_execution_duration_seconds",
                "Wall-clock duration of a successful module execution",
            ),
            ModuleLabels::NAMES,
        )?;

        let module_failures = IntCounterVec::new(
            Opts::new(
                "plaid_module_failures_total",
                "Total number of module executions that returned a non-zero status",
            ),
            ModuleLabels::NAMES,
        )?;

        handle.register(Box::new(computation_percentage.clone()))?;
        handle.register(Box::new(execution_duration_seconds.clone()))?;
        handle.register(Box::new(module_failures.clone()))?;

        Ok(Self {
            computation_percentage,
            execution_duration_seconds,
            module_failures,
        })
    }

    pub fn record_successful_execution(
        &self,
        module: &str,
        computation_used_percentage: f64,
        duration: std::time::Duration,
    ) {
        let labels = ModuleLabels { module };
        self.computation_percentage
            .with_label_values(&labels.values())
            .observe(computation_used_percentage);
        self.execution_duration_seconds
            .with_label_values(&labels.values())
            .observe(duration.as_secs_f64());
    }

    pub fn record_module_failure(&self, module: &str) {
        self.module_failures
            .with_label_values(&ModuleLabels { module }.values())
            .inc();
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
    pub fn register(
        handle: &MetricsHandle,
        pools: &ExecutionThreadPools,
    ) -> Result<(), prometheus::Error> {
        handle.register(Box::new(Self::new(pools)?))
    }

    fn new(pools: &ExecutionThreadPools) -> Result<Self, prometheus::Error> {
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
            QueueLabels::NAMES,
        )?;

        let capacity_percentage = GaugeVec::new(
            Opts::new(
                "plaid_execution_queue_capacity_percentage",
                "Percentage of the execution queue's capacity currently in use",
            ),
            QueueLabels::NAMES,
        )?;

        Ok(Self {
            senders,
            depth,
            capacity_percentage,
        })
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
            let labels = QueueLabels { queue };
            let values = labels.values();
            self.depth.with_label_values(&values).set(depth as i64);
            self.capacity_percentage.with_label_values(&values).set(pct);
        }

        let mut families = self.depth.collect();
        families.extend(self.capacity_percentage.collect());
        families
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExecutorConfig;
    use crate::metrics::{label_pairs, MetricsHandle};

    #[test]
    fn every_label_set_pairs_its_names_with_values() {
        let module = ModuleLabels {
            module: "audit_log",
        };
        assert_eq!(
            label_pairs(ModuleLabels::NAMES, &module.values()),
            [("module", "audit_log")]
        );

        let queue = QueueLabels { queue: "general" };
        assert_eq!(
            label_pairs(QueueLabels::NAMES, &queue.values()),
            [("queue", "general")]
        );
    }

    #[test]
    fn records_module_execution_by_module() -> Result<(), Box<dyn std::error::Error>> {
        let handle = MetricsHandle::new();
        let metrics = ModuleExecutionMetrics::register(&handle)?;

        metrics.record_successful_execution("alpha", 42.0, std::time::Duration::from_millis(500));
        metrics.record_module_failure("alpha");

        let encoded = handle.encode()?;
        for expected in [
            "plaid_module_computation_percentage_used_sum{module=\"alpha\"} 42",
            "plaid_module_execution_duration_seconds_sum{module=\"alpha\"} 0.5",
            "plaid_module_failures_total{module=\"alpha\"} 1",
        ] {
            assert!(encoded.contains(expected), "missing {expected}");
        }

        Ok(())
    }

    #[test]
    fn reports_queue_depth_by_queue() -> Result<(), Box<dyn std::error::Error>> {
        let pools = ExecutionThreadPools::new(&ExecutorConfig {
            execution_threads: 1,
            log_queue_size: 4,
            dedicated_threads: HashMap::new(),
        });
        let handle = MetricsHandle::new();
        QueueMetrics::register(&handle, &pools)?;

        let encoded = handle.encode()?;
        for expected in [
            "plaid_execution_queue_depth{queue=\"general\"} 0",
            "plaid_execution_queue_capacity_percentage{queue=\"general\"} 0",
        ] {
            assert!(encoded.contains(expected), "missing {expected}");
        }

        Ok(())
    }
}
