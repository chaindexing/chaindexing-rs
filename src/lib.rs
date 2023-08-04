mod chains;
mod config;
mod contracts;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;
mod schema;

pub use chains::{Chains, JsonRpcUrl};
pub use config::Config;
pub use contracts::{Contract, ContractAddress, ContractEvent};
pub use ethers::prelude::Chain;
pub use event_handlers::EventHandler;
pub use events::Event;
pub use repos::{PostgresRepo, Repo};

use events_ingester::EventsIngester;

#[cfg(feature = "postgres")]
pub type ChaindexingRepo = PostgresRepo;

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
