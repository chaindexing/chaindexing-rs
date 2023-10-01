use crate::{ChaindexingRepo, Chains, Contract};

#[derive(Clone)]
pub struct Config {
    pub chains: Chains,
    pub repo: ChaindexingRepo,
    pub contracts: Vec<Contract>,
    /// Tolerance for chain re-organization
    pub min_confirmation_count: u8,
    pub blocks_per_batch: u64,
    pub handler_interval_ms: u64,
    pub ingestion_interval_ms: u64,
    pub reset_count: u8,
}

impl Config {
    pub fn new(repo: ChaindexingRepo, chains: Chains) -> Self {
        Self {
            repo,
            chains,
            contracts: vec![],
            min_confirmation_count: 40,
            blocks_per_batch: 20,
            handler_interval_ms: 10000,
            ingestion_interval_ms: 10000,
            reset_count: 0,
        }
    }

    pub fn add_contract(mut self, contract: Contract) -> Self {
        self.contracts.push(contract);

        self
    }

    pub fn reset(mut self, count: u8) -> Self {
        self.reset_count = count;

        self
    }

    pub fn with_min_confirmation_count(mut self, min_confirmation_count: u8) -> Self {
        self.min_confirmation_count = min_confirmation_count;

        self
    }

    pub fn with_blocks_per_batch(mut self, blocks_per_batch: u64) -> Self {
        self.blocks_per_batch = blocks_per_batch;

        self
    }

    pub fn with_handler_interval_ms(mut self, handler_interval_ms: u64) -> Self {
        self.handler_interval_ms = handler_interval_ms;

        self
    }

    pub fn with_ingestion_interval_ms(mut self, ingestion_interval_ms: u64) -> Self {
        self.ingestion_interval_ms = ingestion_interval_ms;

        self
    }
}
