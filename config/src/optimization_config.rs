use nodes::KeepNodeActiveRequest;

#[derive(Clone)]
pub struct OptimizationConfig {
    pub keep_node_active_request: KeepNodeActiveRequest,
    /// Optimization starts after the seconds specified here.
    /// This is the typically the estimated time to complete initial indexing
    /// i.e. the estimated time in seconds for chaindexing to reach
    /// the current block for all chains being indexed.
    pub optimize_after_in_secs: u64,
}
