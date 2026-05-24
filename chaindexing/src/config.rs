use std::collections::HashSet;
use std::sync::Arc;

use alloy::json_abi::Event as AbiEvent;
use tokio::sync::Mutex;

use crate::chain_reorg::{IndexingFinality, MinConfirmationCount, ReorgMode, SideEffectFinality};
use crate::chains::Chain;
use crate::nodes::{self, NodeHeartbeat};
use crate::pruning::PruningConfig;
use crate::runtime_config::{RuntimeConfig, RuntimeConfigError};
use crate::{ChaindexingRepo, Contract};

const DEFAULT_LEADER_LOCK_ID: i64 = 8_841_337_001;

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
    RuntimeConfig(RuntimeConfigError),
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
            ConfigError::RuntimeConfig(error) => {
                write!(f, "Invalid runtime config: {error}")
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
    pub ingester_concurrency: u32,
    pub handler_concurrency: u32,
    pub runtime_config: RuntimeConfig,
    node_election_rate_ms: Option<u64>,
    pub reset_count: u64,
    pub(crate) reset_including_side_effects_count: u64,
    pub reset_queries: Vec<String>,
    pub shared_state: Option<Arc<Mutex<SharedState>>>,
    pub max_concurrent_node_count: u16,
    pub optimization_config: Option<OptimizationConfig>,
    pub(crate) pruning_config: Option<PruningConfig>,
    pub(crate) leader_lock_id: i64,
    pub(crate) reorg_mode: ReorgMode,
    pub(crate) indexing_finality: IndexingFinality,
    pub(crate) side_effect_finality: SideEffectFinality,
}

impl<SharedState: Sync + Send + Clone> Config<SharedState> {
    pub fn new(repo: ChaindexingRepo) -> Self {
        let runtime_config = RuntimeConfig::legacy_compatible();
        Self {
            repo,
            chains: vec![],
            contracts: vec![],
            min_confirmation_count: MinConfirmationCount::new(40),
            blocks_per_batch: runtime_config.blocks_per_batch_value(),
            handler_rate_ms: runtime_config.handler_poll_interval_ms_value(),
            ingestion_rate_ms: runtime_config.ingestion_poll_interval_ms_value(),
            chain_concurrency: runtime_config.limits_ref().max_ingester_workers_value(),
            ingester_concurrency: runtime_config.limits_ref().max_ingester_workers_value(),
            handler_concurrency: runtime_config.limits_ref().max_handler_workers_value(),
            runtime_config,
            node_election_rate_ms: None,
            reset_count: 0,
            reset_including_side_effects_count: 0,
            reset_queries: vec![],
            shared_state: None,
            max_concurrent_node_count: nodes::DEFAULT_MAX_CONCURRENT_NODE_COUNT,
            optimization_config: None,
            pruning_config: None,
            leader_lock_id: DEFAULT_LEADER_LOCK_ID,
            reorg_mode: ReorgMode::Realtime,
            indexing_finality: IndexingFinality::LatestWithConfirmations(0),
            side_effect_finality: SideEffectFinality::Safe,
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

    /// Uses a preset reorg/finality posture.
    ///
    /// `Realtime` preserves the low-latency default while still repairing reorgs
    /// by canonical block hash. `Balanced` uses `safe` when supported, and
    /// `FinalityFirst` uses `finalized` when supported.
    pub fn with_reorg_mode(mut self, reorg_mode: ReorgMode) -> Self {
        self.reorg_mode = reorg_mode;
        self.indexing_finality = reorg_mode.indexing_finality(self.min_confirmation_count);
        self.side_effect_finality = reorg_mode.side_effect_finality();

        self
    }

    /// Overrides the preset's event ingestion finality policy.
    pub fn with_indexing_finality(mut self, indexing_finality: IndexingFinality) -> Self {
        self.indexing_finality = indexing_finality;

        self
    }

    /// Overrides the preset's durable side-effect dispatch finality policy.
    pub fn with_side_effect_finality(mut self, side_effect_finality: SideEffectFinality) -> Self {
        self.side_effect_finality = side_effect_finality;

        self
    }

    /// Advanced config: how many blocks per batch should be ingested and handled.
    /// The default is kept legacy-compatible; prefer `RuntimeConfig` profiles for new applications.
    pub fn with_blocks_per_batch(mut self, blocks_per_batch: u64) -> Self {
        self.blocks_per_batch = blocks_per_batch;
        self.runtime_config = self.runtime_config.blocks_per_batch(blocks_per_batch);

        self
    }

    /// Advanced config: how often event handler processes should run.
    /// The default is kept legacy-compatible; prefer `RuntimeConfig` profiles for new applications.
    pub fn with_handler_rate_ms(mut self, handler_rate_ms: u64) -> Self {
        self.handler_rate_ms = handler_rate_ms;
        self.runtime_config = self.runtime_config.handler_poll_interval_ms(handler_rate_ms);

        self
    }

    /// Advanced config: how often event ingester processes should run.
    /// The default is kept legacy-compatible; prefer `RuntimeConfig` profiles for new applications.
    pub fn with_ingestion_rate_ms(mut self, ingestion_rate_ms: u64) -> Self {
        self.ingestion_rate_ms = ingestion_rate_ms;
        self.runtime_config = self.runtime_config.ingestion_poll_interval_ms(ingestion_rate_ms);

        self
    }

    /// Configures number of chain batches to be processed concurrently
    pub fn with_chain_concurrency(mut self, chain_concurrency: u32) -> Self {
        self.chain_concurrency = chain_concurrency;
        self.ingester_concurrency = chain_concurrency;
        self.handler_concurrency = chain_concurrency;

        let limits = self
            .runtime_config
            .limits_ref()
            .clone()
            .max_ingester_workers(chain_concurrency)
            .max_handler_workers(chain_concurrency);
        self.runtime_config = self.runtime_config.limits(limits);

        self
    }

    /// Configures maximum ingestion workers. This is preferred over
    /// `with_chain_concurrency` when ingestion and handler pressure differ.
    pub fn with_ingester_concurrency(mut self, ingester_concurrency: u32) -> Self {
        self.ingester_concurrency = ingester_concurrency;
        self.chain_concurrency = self.ingester_concurrency.max(self.handler_concurrency);

        let limits = self
            .runtime_config
            .limits_ref()
            .clone()
            .max_ingester_workers(ingester_concurrency);
        self.runtime_config = self.runtime_config.limits(limits);

        self
    }

    /// Configures maximum handler workers. Effective handler parallelism is still
    /// limited by state ordering partitions.
    pub fn with_handler_concurrency(mut self, handler_concurrency: u32) -> Self {
        self.handler_concurrency = handler_concurrency;
        self.chain_concurrency = self.ingester_concurrency.max(self.handler_concurrency);

        let limits = self
            .runtime_config
            .limits_ref()
            .clone()
            .max_handler_workers(handler_concurrency);
        self.runtime_config = self.runtime_config.limits(limits);

        self
    }

    /// Configures the runtime with behavior-oriented profiles and explicit
    /// resource/RPC limits. Existing low-level setters still work for
    /// compatibility, but this is the preferred API for new applications.
    pub fn with_runtime_config(mut self, runtime_config: RuntimeConfig) -> Self {
        self.blocks_per_batch = runtime_config.blocks_per_batch_value();
        self.handler_rate_ms = runtime_config.handler_poll_interval_ms_value();
        self.ingestion_rate_ms = runtime_config.ingestion_poll_interval_ms_value();
        self.ingester_concurrency = runtime_config.limits_ref().max_ingester_workers_value();
        self.handler_concurrency = runtime_config.limits_ref().max_handler_workers_value();
        self.chain_concurrency = self.ingester_concurrency.max(self.handler_concurrency);
        self.runtime_config = runtime_config;

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

    /// Sets the Postgres advisory lock id used for leader election.
    pub fn with_leader_lock_id(mut self, leader_lock_id: i64) -> Self {
        self.leader_lock_id = leader_lock_id;

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

    pub(crate) fn effective_ingester_concurrency(&self) -> u32 {
        let rpc = self.runtime_config.rpc_ref();
        let max_rpc_in_flight = rpc.max_in_flight_value().max(1);
        let max_rpc_per_chain = rpc.max_per_chain_value().max(1);
        let rpc_limited_workers = max_rpc_in_flight / max_rpc_per_chain;

        self.ingester_concurrency.max(1).min(rpc_limited_workers.max(1))
    }

    pub(crate) fn effective_handler_concurrency(&self) -> u32 {
        self.handler_concurrency.max(1)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_runtime_fields()?;
        self.runtime_config.validate().map_err(ConfigError::RuntimeConfig)?;

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
                if let Err(error) = AbiEvent::parse(abi) {
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

    fn validate_runtime_fields(&self) -> Result<(), ConfigError> {
        for (field, value) in [
            ("blocks_per_batch", self.blocks_per_batch),
            ("handler_rate_ms", self.handler_rate_ms),
            ("ingestion_rate_ms", self.ingestion_rate_ms),
            ("chain_concurrency", self.chain_concurrency as u64),
            ("ingester_concurrency", self.ingester_concurrency as u64),
            ("handler_concurrency", self.handler_concurrency as u64),
        ] {
            if value == 0 {
                return Err(ConfigError::RuntimeConfig(RuntimeConfigError::new(
                    field,
                    "must be greater than zero",
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::PureHandlerContext;
    use crate::{ChainId, EventHandler, RpcPolicy, RuntimeConfig, RuntimeLimits};

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

    fn valid_contract() -> Contract<()> {
        Contract::<()>::new("ERC721").add_event_handler(TestHandler(
            "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)",
        ))
    }

    fn valid_config() -> Config<()> {
        Config::new(repo())
            .add_chain(Chain::mainnet("http://localhost:8545"))
            .add_contract(valid_contract())
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

    #[test]
    fn rejects_zero_concurrency() {
        let config = valid_config().with_chain_concurrency(0);

        assert!(matches!(
            config.validate(),
            Err(ConfigError::RuntimeConfig(_))
        ));
    }

    #[test]
    fn applies_runtime_config_to_legacy_runtime_fields() {
        let config: Config<()> = Config::new(repo()).with_runtime_config(
            RuntimeConfig::backfill()
                .limits(RuntimeLimits::throughput().max_ingester_workers(11).max_handler_workers(7))
                .rpc(RpcPolicy::throughput().max_in_flight(44).max_per_chain(11))
                .blocks_per_batch(1_234)
                .ingestion_poll_interval_ms(333)
                .handler_poll_interval_ms(222),
        );

        assert_eq!(config.blocks_per_batch, 1_234);
        assert_eq!(config.ingestion_rate_ms, 333);
        assert_eq!(config.handler_rate_ms, 222);
        assert_eq!(config.ingester_concurrency, 11);
        assert_eq!(config.handler_concurrency, 7);
        assert_eq!(config.chain_concurrency, 11);
        assert_eq!(config.runtime_config.rpc_ref().max_per_chain_value(), 11);
    }

    #[test]
    fn legacy_setters_keep_runtime_config_in_sync() {
        let config: Config<()> = Config::new(repo())
            .with_blocks_per_batch(999)
            .with_ingestion_rate_ms(888)
            .with_handler_rate_ms(777)
            .with_chain_concurrency(6);

        assert_eq!(config.runtime_config.blocks_per_batch_value(), 999);
        assert_eq!(
            config.runtime_config.ingestion_poll_interval_ms_value(),
            888
        );
        assert_eq!(config.runtime_config.handler_poll_interval_ms_value(), 777);
        assert_eq!(
            config.runtime_config.limits_ref().max_ingester_workers_value(),
            6
        );
        assert_eq!(
            config.runtime_config.limits_ref().max_handler_workers_value(),
            6
        );
    }

    #[test]
    fn separate_concurrency_setters_keep_legacy_alias_in_sync() {
        let config: Config<()> =
            Config::new(repo()).with_ingester_concurrency(2).with_handler_concurrency(7);

        assert_eq!(config.chain_concurrency, 7);
        assert_eq!(
            config.runtime_config.limits_ref().max_ingester_workers_value(),
            2
        );
        assert_eq!(
            config.runtime_config.limits_ref().max_handler_workers_value(),
            7
        );
    }

    #[test]
    fn effective_ingester_concurrency_never_exceeds_global_rpc_budget() {
        let config: Config<()> = Config::new(repo()).with_runtime_config(
            RuntimeConfig::backfill()
                .limits(RuntimeLimits::throughput().max_ingester_workers(10))
                .rpc(RpcPolicy::throughput().max_in_flight(17).max_per_chain(8)),
        );

        assert_eq!(config.effective_ingester_concurrency(), 2);
    }

    #[test]
    fn reorg_mode_sets_indexing_and_side_effect_finality() {
        let config: Config<()> = Config::new(repo()).with_reorg_mode(ReorgMode::Balanced);

        assert_eq!(config.reorg_mode, ReorgMode::Balanced);
        assert_eq!(config.indexing_finality, IndexingFinality::Safe);
        assert_eq!(config.side_effect_finality, SideEffectFinality::Safe);
    }

    #[test]
    fn finality_overrides_keep_preset_visible() {
        let config: Config<()> = Config::new(repo())
            .with_reorg_mode(ReorgMode::FinalityFirst)
            .with_indexing_finality(IndexingFinality::LatestWithConfirmations(6))
            .with_side_effect_finality(SideEffectFinality::Confirmations(12));

        assert_eq!(config.reorg_mode, ReorgMode::FinalityFirst);
        assert_eq!(
            config.indexing_finality,
            IndexingFinality::LatestWithConfirmations(6)
        );
        assert_eq!(
            config.side_effect_finality,
            SideEffectFinality::Confirmations(12)
        );
    }
}
