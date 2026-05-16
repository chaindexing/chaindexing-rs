use std::collections::HashSet;
use std::sync::Arc;

use ethers::abi::HumanReadableParser;
use tokio::sync::Mutex;

use crate::chain_reorg::MinConfirmationCount;
use crate::chains::Chain;
use crate::nodes::{self, NodeHeartbeat};
use crate::pruning::PruningConfig;
use crate::{ChaindexingRepo, Contract};

#[derive(Clone, Eq, PartialEq)]
pub enum ConfigError {
    NoContract,
    NoChain,
    DuplicateContractName(String),
    DuplicateContractAddress {
        chain_id: i64,
        address: String,
    },
    InvalidEventAbi {
        contract_name: String,
        abi: String,
        error: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoContract => {
                write!(f, "At least one contract is required")
            }
            ConfigError::NoChain => {
                write!(f, "At least one chain is required")
            }
            ConfigError::DuplicateContractName(name) => {
                write!(f, "Duplicate contract name `{name}`")
            }
            ConfigError::DuplicateContractAddress { chain_id, address } => {
                write!(
                    f,
                    "Duplicate contract address `{address}` on chain `{chain_id}`"
                )
            }
            ConfigError::InvalidEventAbi {
                contract_name,
                abi,
                error,
            } => {
                write!(
                    f,
                    "Invalid event ABI `{abi}` on contract `{contract_name}`: {error}"
                )
            }
        }
    }
}

impl std::fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::error::Error for ConfigError {}

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

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.contracts.is_empty() {
            return Err(ConfigError::NoContract);
        }

        if self.chains.is_empty() {
            return Err(ConfigError::NoChain);
        }

        let mut contract_names = HashSet::new();
        let mut contract_addresses = HashSet::new();

        for contract in &self.contracts {
            if !contract_names.insert(contract.name.clone()) {
                return Err(ConfigError::DuplicateContractName(contract.name.clone()));
            }

            for contract_address in &contract.addresses {
                let key = (contract_address.chain_id, contract_address.address.clone());
                if !contract_addresses.insert(key) {
                    return Err(ConfigError::DuplicateContractAddress {
                        chain_id: contract_address.chain_id,
                        address: contract_address.address.clone(),
                    });
                }
            }

            for abi in contract.get_event_abis() {
                if let Err(error) = HumanReadableParser::parse_event(abi) {
                    return Err(ConfigError::InvalidEventAbi {
                        contract_name: contract.name.clone(),
                        abi: abi.to_string(),
                        error: error.to_string(),
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::PureHandlerContext;
    use crate::{ChainId, EventHandler};

    struct TestHandler(&'static str);

    #[crate::augmenting_std::async_trait]
    impl EventHandler for TestHandler {
        fn abi(&self) -> &'static str {
            self.0
        }

        async fn handle_event<'a, 'b>(&self, _context: PureHandlerContext<'a, 'b>) {}
    }

    fn repo() -> ChaindexingRepo {
        ChaindexingRepo::new("postgres://localhost/chaindexing")
    }

    #[test]
    fn rejects_duplicate_contract_names() {
        let config = Config::new(repo())
            .add_chain(Chain::mainnet("http://localhost:8545"))
            .add_contract(Contract::<()>::new("ERC721"))
            .add_contract(Contract::<()>::new("ERC721"));

        assert_eq!(
            config.validate(),
            Err(ConfigError::DuplicateContractName("ERC721".to_string()))
        );
    }

    #[test]
    fn rejects_duplicate_contract_addresses_per_chain() {
        let address = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
        let config = Config::new(repo())
            .add_chain(Chain::mainnet("http://localhost:8545"))
            .add_contract(Contract::<()>::new("ERC721").add_address(address, &ChainId::Mainnet, 1))
            .add_contract(Contract::<()>::new("ERC20").add_address(address, &ChainId::Mainnet, 1));

        assert_eq!(
            config.validate(),
            Err(ConfigError::DuplicateContractAddress {
                chain_id: ChainId::Mainnet as i64,
                address: address.to_lowercase(),
            })
        );
    }

    #[test]
    fn rejects_invalid_event_abis() {
        let config = Config::new(repo())
            .add_chain(Chain::mainnet("http://localhost:8545"))
            .add_contract(Contract::<()>::new("ERC721").add_event_handler(TestHandler("Transfer")));

        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidEventAbi { .. })
        ));
    }
}
