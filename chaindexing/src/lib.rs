mod chains;
mod config;
mod contract_states;
mod contracts;
mod diesels;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;

pub use chains::Chains;
pub use config::Config;
pub use contract_states::{ContractState, ContractStateError, ContractStateMigrations};
pub use contracts::{Contract, ContractAddress, ContractEvent};
pub use diesel;
pub use diesel::prelude::QueryableByName;
pub use ethers::prelude::Chain;
pub use event_handlers::{AllEventHandlers, EventHandler, EventHandlers};
pub use events::Event;
pub use events_ingester::{EventsIngester, EventsIngesterJsonRpc};
pub use repos::*;

pub use ethers::prelude::{Address, U256, U64};

#[cfg(feature = "postgres")]
pub use repos::{PostgresRepo, PostgresRepoConn, PostgresRepoPool};

#[cfg(feature = "postgres")]
pub type ChaindexingRepo = PostgresRepo;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoPool = PostgresRepoPool;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoConn<'a> = PostgresRepoConn<'a>;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoRawQueryClient = PostgresRepoRawQueryClient;

#[cfg(feature = "postgres")]
pub use repos::PostgresRepoAsyncConnection as ChaindexingRepoAsyncConnection;

pub struct Chaindexing;

impl Chaindexing {
    pub async fn index_states(config: &Config) -> Result<(), ()> {
        Self::setup(config).await?;
        EventsIngester::start(config);
        EventHandlers::start(config);
        Ok(())
    }

    pub async fn setup(config: &Config) -> Result<(), ()> {
        let raw_query_client = config.repo.get_raw_query_client().await;
        Self::run_internal_migrations(&raw_query_client).await;
        Self::run_migrations_for_contract_states(&raw_query_client, &config.contracts).await;

        let pool = config.repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;
        Self::create_initial_contract_addresses(&mut conn, &config.contracts).await;

        Ok(())
    }

    pub async fn run_internal_migrations(raw_query_client: &ChaindexingRepoRawQueryClient) {
        ChaindexingRepo::migrate(
            &raw_query_client,
            ChaindexingRepo::get_all_internal_migrations(),
        )
        .await;
    }

    pub async fn run_migrations_for_contract_states(
        raw_query_client: &ChaindexingRepoRawQueryClient,
        contracts: &Vec<Contract>,
    ) {
        let states = contracts.iter().flat_map(|c| c.state_migrations.clone());

        for state in states {
            ChaindexingRepo::migrate(raw_query_client, state.migrations()).await;
        }
    }

    pub async fn create_initial_contract_addresses<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contracts: &Vec<Contract>,
    ) {
        let contract_addresses =
            contracts.clone().into_iter().map(|c| c.addresses).flatten().collect();

        ChaindexingRepo::create_contract_addresses(conn, &contract_addresses).await;
    }
}
