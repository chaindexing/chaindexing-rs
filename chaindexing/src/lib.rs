mod chains;
mod config;
mod contracts;
mod diesel;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;

pub use chains::Chains;
pub use config::Config;
pub use contracts::{Contract, ContractAddress, ContractEvent};
pub use ethers::prelude::Chain;
pub use event_handlers::EventHandler;
pub use events::Event;
pub use events_ingester::{EventsIngester, EventsIngesterJsonRpc};
pub use repos::*;

#[cfg(feature = "postgres")]
pub use repos::{PostgresRepo, PostgresRepoConn, PostgresRepoPool};

#[cfg(feature = "postgres")]
pub type ChaindexingRepo = PostgresRepo;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoPool = PostgresRepoPool;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoConn<'a> = PostgresRepoConn<'a>;

#[cfg(feature = "postgres")]
pub use repos::PostgresRepoAsyncConnection as ChaindexingRepoAsyncConnection;

pub struct Chaindexing;

impl Chaindexing {
    pub async fn index_states(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
        let pool = config.repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;
        config.repo.migrate(&mut conn).await;

        Self::create_initial_contract_addresses(&mut conn, &config.contracts).await;

        EventsIngester::start(config);

        Ok(())
    }

    pub async fn create_initial_contract_addresses<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contracts: &Vec<Contract>,
    ) {
        let contract_addresses = contracts
            .clone()
            .into_iter()
            .map(|c| c.addresses)
            .flatten()
            .collect();

        ChaindexingRepo::create_contract_addresses(conn, &contract_addresses).await;
    }
}
