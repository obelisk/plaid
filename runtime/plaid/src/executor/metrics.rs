use std::collections::HashMap;

use crossbeam_channel::Sender;
use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{GaugeVec, IntGaugeVec, Opts};

use super::thread_pools::ExecutionThreadPools;
use super::Message;

/// Reports depth and percentage capacity of each execution queue. Values are
/// read from the live channel senders at scrape time, so there are no writers.
pub struct QueueMetrics {
    /// One sender per queue: "general" + each dedicated log type.
    senders: HashMap<String, Sender<Message>>,
    depth: IntGaugeVec,
    capacity_percentage: GaugeVec,
}

impl QueueMetrics {
    pub fn from_pools(pools: &ExecutionThreadPools) -> Self {
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
