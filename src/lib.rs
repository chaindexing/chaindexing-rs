mod chains;
mod contracts;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;
mod schema;

pub use chains::{Chains, JsonRpcUrl};
pub use contracts::{Contract, ContractAddress, ContractEvent};
pub use ethers::prelude::Chain;
pub use event_handlers::EventHandler;
pub use events::Event;
pub use repos::{PostgresRepo, Repo};

use events_ingester::EventsIngester;

#[cfg(feature = "postgres")]
pub type ChaindexingRepo = PostgresRepo;

#[derive(Clone)]
pub struct Config {
    pub chains: Chains,
    pub repo: ChaindexingRepo,
    pub contracts: Vec<Contract>,
    pub blocks_per_batch: u64,
    pub handler_interval_ms: u64,
    pub ingestion_interval_ms: u64,
}

impl Config {
    pub fn new(repo: ChaindexingRepo, chains: Chains, contracts: Vec<Contract>) -> Self {
        Self {
            repo,
            chains,
            contracts,
            blocks_per_batch: 20,
            handler_interval_ms: 10000,
            ingestion_interval_ms: 10000,
        }
    }

    pub fn with_blocks_per_batch(&self, blocks_per_batch: u64) -> Self {
        Self {
            blocks_per_batch,
            ..self.clone()
        }
    }

    pub fn with_handler_interval_ms(&self, handler_interval_ms: u64) -> Self {
        Self {
            handler_interval_ms,
            ..self.clone()
        }
    }

    pub fn with_ingestion_interval_ms(&self, ingestion_interval_ms: u64) -> Self {
        Self {
            ingestion_interval_ms,
            ..self.clone()
        }
    }
}
pub struct Chaindexing;

impl Chaindexing {
    pub async fn index_states(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
        Self::create_initial_contract_addresses(config).await;

        EventsIngester::start(config);

        Ok(())
    }

    async fn create_initial_contract_addresses<'a>(config: &Config) {
        let contract_addresses = config
            .contracts
            .clone()
            .into_iter()
            .map(|c| c.addresses)
            .flatten()
            .collect();

        config
            .repo
            .create_contract_addresses(&contract_addresses)
            .await;
    }
}
