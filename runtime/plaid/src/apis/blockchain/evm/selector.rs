use crate::apis::blockchain::evm::NodeConfig;
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Selection strategy for choosing EVM nodes
pub enum SelectionStrategy {
    /// Select nodes in a round-robin fashion
    RoundRobin { current_index: AtomicUsize },
    /// Select nodes randomly
    Random,
}

/// Selector for EVM nodes based on a selection strategy
pub struct NodeSelector {
    /// The chain ID associated with this selector
    pub id: u64,
    /// The available nodes for this chain
    nodes: Vec<NodeConfig>,
    /// The selection strategy to use
    selection_state: SelectionStrategy,
}

impl NodeSelector {
    pub fn new(id: u64, nodes: Vec<NodeConfig>, selection_state: SelectionStrategy) -> Self {
        Self {
            id,
            nodes,
            selection_state,
        }
    }
}

impl NodeSelector {
    /// Select a node based on the selection strategy
    pub fn select_node(&self) -> Option<NodeConfig> {
        if self.nodes.is_empty() {
            return None;
        }

        match &self.selection_state {
            SelectionStrategy::RoundRobin { current_index } => {
                // Get current index without advancing - we only advance on failure
                let index = current_index.load(Ordering::Relaxed) % self.nodes.len();
                self.nodes.get(index).cloned()
            }
            SelectionStrategy::Random => {
                let mut rng = rand::thread_rng();
                self.nodes.choose(&mut rng).cloned()
            }
        }
    }

    /// Mark the currently selected node as failed and advance to the next node.
    ///
    /// For RoundRobin: advances the index, effectively "skipping" the failed node
    /// and moving it to the "back of the line" in the rotation.
    ///
    /// For Random: this is a no-op since random selection doesn't maintain order.
    pub fn mark_current_node_failed(&self) {
        match &self.selection_state {
            SelectionStrategy::RoundRobin { current_index } => {
                // Simply advance the index - this effectively moves the failed node
                // to the "back of the line" because:
                // 1. We skip over it by advancing
                // 2. We only return to it after cycling through all other nodes
                if !self.nodes.is_empty() {
                    current_index.fetch_add(1, Ordering::Relaxed);
                }
            }
            SelectionStrategy::Random => {
                // Random strategy doesn't need special failure handling
                // as each selection is independent
            }
        }
    }
}
