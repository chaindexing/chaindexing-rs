use std::{collections::HashMap, sync::Arc};

use ethers::types::Chain;
use tokio::sync::Mutex;

use crate::{ChaindexingRepo, Chains, Contract, MinConfirmationCount};

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
#[derive(Clone)]
pub struct Config<SharedState: Sync + Send + Clone> {
    pub chains: Chains,
    pub repo: ChaindexingRepo,
    pub contracts: Vec<Contract<SharedState>>,
    pub min_confirmation_count: MinConfirmationCount,
    pub blocks_per_batch: u64,
    pub handler_rate_ms: u64,
    pub ingestion_rate_ms: u64,
    pub reset_count: u8,
    pub shared_state: Option<Arc<Mutex<SharedState>>>,
}

impl<SharedState: Sync + Send + Clone> Config<SharedState> {
    pub fn new(repo: ChaindexingRepo, initial_state: Option<SharedState>) -> Self {
        Self {
            repo,
            chains: HashMap::new(),
            contracts: vec![],
            min_confirmation_count: MinConfirmationCount::new(40),
            blocks_per_batch: 10000,
            handler_rate_ms: 4000,
            ingestion_rate_ms: 4000,
            reset_count: 0,
            shared_state: initial_state.map(|s| Arc::new(Mutex::new(s))),
        }
    }

    pub fn add_chain(mut self, chain: Chain, json_rpc_url: &str) -> Self {
        self.chains.insert(chain, json_rpc_url.to_string());

        self
    }

    pub fn add_contract(mut self, contract: Contract<SharedState>) -> Self {
        self.contracts.push(contract);

        self
    }

    pub fn reset(mut self, count: u8) -> Self {
        self.reset_count = count;

        self
    }

    pub fn with_min_confirmation_count(mut self, min_confirmation_count: u8) -> Self {
        self.min_confirmation_count = MinConfirmationCount::new(min_confirmation_count);

        self
    }

    pub fn with_blocks_per_batch(mut self, blocks_per_batch: u64) -> Self {
        self.blocks_per_batch = blocks_per_batch;

        self
    }

    pub fn with_handler_rate_ms(mut self, handler_rate_ms: u64) -> Self {
        self.handler_rate_ms = handler_rate_ms;

        self
    }

    pub fn with_ingestion_rate_ms(mut self, ingestion_rate_ms: u64) -> Self {
        self.ingestion_rate_ms = ingestion_rate_ms;

        self
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
