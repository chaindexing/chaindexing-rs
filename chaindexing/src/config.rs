use std::sync::Arc;

use tokio::sync::Mutex;

use crate::chain_reorg::MinConfirmationCount;
use crate::chains::Chain;
use crate::nodes::{self, NodeHeartbeat};
use crate::pruning::PruningConfig;
use crate::{ChaindexingRepo, Contract};

pub enum ConfigError {
    NoContract,
    NoChain,
}

impl std::fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoContract => {
                write!(f, "At least one contract is required")
            }
            ConfigError::NoChain => {
                write!(f, "At least one chain is required")
            }
        }
    }
}

/// Used to configure managing a chaindexing's node heartbeat
/// to cut down JSON-RPC's (Alchemy, Infura, etc.) cost.
#[derive(Clone, Debug)]
pub struct OptimizationConfig {
    pub(crate) node_heartbeat: NodeHeartbeat,
    pub(crate) start_after_in_secs: u64,
}

impl OptimizationConfig {
    /// Optimization starts after the seconds specified here.
    /// This is the typically the estimated time to complete initial indexing
    /// i.e. the estimated time in seconds for chaindexing to reach
    /// the current block for all chains being indexed.
    pub fn new(node_heartbeat: &NodeHeartbeat, start_after_in_secs: u64) -> Self {
        Self {
            node_heartbeat: node_heartbeat.clone(),
            start_after_in_secs,
        }
    }
}

/// Configuration for indexing states
#[derive(Clone, Debug)]
pub struct Config<SharedState: Sync + Send + Clone> {
    pub chains: Vec<Chain>,
    pub repo: ChaindexingRepo,
    pub contracts: Vec<Contract<SharedState>>,
    pub(crate) min_confirmation_count: MinConfirmationCount,
    pub blocks_per_batch: u64,
    pub handler_rate_ms: u64,
    pub ingestion_rate_ms: u64,
    pub chain_concurrency: u32,
    node_election_rate_ms: Option<u64>,
    pub reset_count: u64,
    pub(crate) reset_including_side_effects_count: u64,
    pub reset_queries: Vec<String>,
    pub shared_state: Option<Arc<Mutex<SharedState>>>,
    pub max_concurrent_node_count: u16,
    pub optimization_config: Option<OptimizationConfig>,
    pub(crate) pruning_config: Option<PruningConfig>,
}

impl<SharedState: Sync + Send + Clone> Config<SharedState> {
    pub fn new(repo: ChaindexingRepo) -> Self {
        Self {
            repo,
            chains: vec![],
            contracts: vec![],
            min_confirmation_count: MinConfirmationCount::new(40),
            blocks_per_batch: 450,
            handler_rate_ms: 4_000,
            ingestion_rate_ms: 20_000,
            chain_concurrency: 4,
            node_election_rate_ms: None,
            reset_count: 0,
            reset_including_side_effects_count: 0,
            reset_queries: vec![],
            shared_state: None,
            max_concurrent_node_count: nodes::DEFAULT_MAX_CONCURRENT_NODE_COUNT,
            optimization_config: None,
            pruning_config: None,
        }
    }

    // Includes chain in config
    pub fn add_chain(mut self, chain: Chain) -> Self {
        self.chains.push(chain);

        self
    }

    // Includes contract in config
    pub fn add_contract(mut self, contract: Contract<SharedState>) -> Self {
        self.contracts.push(contract);

        self
    }

    /// Allows managing derived app states (derived from indexed states)
    pub fn add_reset_query(mut self, reset_query: &str) -> Self {
        self.reset_queries.push(reset_query.to_string());

        self
    }

    /// Restarts indexing from scratch for EventHandlers. SideEffectHandlers
    /// will not run if they ran already
    pub fn reset(mut self, count: u64) -> Self {
        self.reset_count = count;

        self
    }

    /// Restarts indexing from scratch for all Handlers. SideEffectHandlers
    /// will RUN even if they ran already
    pub fn reset_including_side_effects_dangerously(mut self, count: u64) -> Self {
        self.reset_including_side_effects_count = count;

        self
    }

    /// Defines the initial state for side effect handlers
    pub fn with_initial_state(mut self, initial_state: SharedState) -> Self {
        self.shared_state = Some(Arc::new(Mutex::new(initial_state)));

        self
    }

    /// The minimum confirmation count for detecting chain-reorganizations or uncled blocks
    pub fn with_min_confirmation_count(mut self, min_confirmation_count: u8) -> Self {
        self.min_confirmation_count = MinConfirmationCount::new(min_confirmation_count);

        self
    }

    /// Advance config: How many blocks per batch should be ingested and handled.
    /// Default is 8_000
    pub fn with_blocks_per_batch(mut self, blocks_per_batch: u64) -> Self {
        self.blocks_per_batch = blocks_per_batch;

        self
    }

    /// Advance config: How often should the events handlers processes run.
    /// Default is 4_000
    pub fn with_handler_rate_ms(mut self, handler_rate_ms: u64) -> Self {
        self.handler_rate_ms = handler_rate_ms;

        self
    }

    /// Advance config:  How often should the events ingester processes run.
    /// Default is 20_000
    pub fn with_ingestion_rate_ms(mut self, ingestion_rate_ms: u64) -> Self {
        self.ingestion_rate_ms = ingestion_rate_ms;

        self
    }

    /// Configures number of chain batches to be processed concurrently
    pub fn with_chain_concurrency(mut self, chain_concurrency: u32) -> Self {
        self.chain_concurrency = chain_concurrency;

        self
    }

    pub fn with_node_election_rate_ms(mut self, node_election_rate_ms: u64) -> Self {
        self.node_election_rate_ms = Some(node_election_rate_ms);

        self
    }

    pub fn with_max_concurrent_node_count(mut self, max_concurrent_node_count: u16) -> Self {
        self.max_concurrent_node_count = max_concurrent_node_count;

        self
    }

    /// Deletes stale events and related-internal data
    pub fn with_pruning(mut self) -> Self {
        self.pruning_config = Some(Default::default());

        self
    }

    pub fn with_prune_n_blocks_away(mut self, prune_n_blocks_away: u64) -> Self {
        self.pruning_config = Some(PruningConfig {
            prune_n_blocks_away,
            ..self.pruning_config.unwrap_or_default()
        });

        self
    }

    pub fn with_prune_interval(mut self, prune_interval: u64) -> Self {
        self.pruning_config = Some(PruningConfig {
            prune_interval,
            ..self.pruning_config.unwrap_or_default()
        });

        self
    }

    /// This enables optimization for indexing with the CAVEAT that you have to
    /// manually keep chaindexing alive e.g. when a user enters certain pages
    /// in your DApp
    pub fn enable_optimization(mut self, optimization_config: &OptimizationConfig) -> Self {
        self.optimization_config = Some(optimization_config.clone());

        self
    }
    pub fn is_optimization_enabled(&self) -> bool {
        self.optimization_config.is_some()
    }

    pub(super) fn get_node_election_rate_ms(&self) -> u64 {
        self.node_election_rate_ms.unwrap_or(self.ingestion_rate_ms)
    }

    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        if self.contracts.is_empty() {
            Err(ConfigError::NoContract)
        } else if self.chains.is_empty() {
            Err(ConfigError::NoChain)
        } else {
            Ok(())
        }
    }
}
