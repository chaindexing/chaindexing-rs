mod chains;
mod config;
mod contracts;
mod diesel;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;

pub use chains::{Chains, JsonRpcUrl};
pub use config::Config;
pub use contracts::{Contract, ContractAddress, ContractEvent};
pub use ethers::prelude::Chain;
pub use event_handlers::EventHandler;
pub use events::Event;
pub use repos::{PostgresRepo, PostgresRepoConn, PostgresRepoPool, Repo};

use events_ingester::EventsIngester;

#[cfg(feature = "postgres")]
pub type ChaindexingRepo = PostgresRepo;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoPool = PostgresRepoPool;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoConn<'a> = PostgresRepoConn<'a>;

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

        let pool = &config.repo.get_pool().await;
        let mut conn = ChaindexingRepo::get_conn(pool).await;
        ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;
    }
}
