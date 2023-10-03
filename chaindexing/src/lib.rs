mod chain_reorg;
mod chains;
mod config;
mod contract_states;
mod contracts;
mod diesels;
mod event_handlers;
mod events;
mod events_ingester;
mod repos;
mod reset_counts;

pub use chain_reorg::{MinConfirmationCount, ReorgedBlock, ReorgedBlocks, UnsavedReorgedBlock};
pub use chains::Chains;
pub use config::Config;
pub use contract_states::{ContractState, ContractStateMigrations, ContractStates};
pub use contracts::{Contract, ContractAddress, ContractEvent, Contracts};
pub use diesel;
pub use diesel::prelude::QueryableByName;
pub use ethers::prelude::Chain;
pub use event_handlers::{EventHandler, EventHandlerContext as EventContext, EventHandlers};
pub use events::Event;
pub use events_ingester::{EventsIngester, EventsIngesterJsonRpc};
pub use repos::*;
pub use reset_counts::ResetCount;

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
pub type ChaindexingRepoRawQueryTxnClient<'a> = PostgresRepoRawQueryTxnClient<'a>;

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
        let Config {
            repo,
            contracts,
            reset_count,
            ..
        } = config;

        let client = repo.get_raw_query_client().await;
        let pool = repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;

        Self::run_migrations_for_resets(&client).await;
        Self::maybe_reset(reset_count, contracts, &client, &mut conn).await;
        Self::run_internal_migrations(&client).await;
        Self::run_migrations_for_contract_states(&client, contracts).await;
        Self::create_initial_contract_addresses(&mut conn, contracts).await;

        Ok(())
    }

    pub async fn maybe_reset<'a>(
        reset_count: &u8,
        contracts: &Vec<Contract>,
        client: &ChaindexingRepoRawQueryClient,
        conn: &mut ChaindexingRepoConn<'a>,
    ) {
        let reset_count = *reset_count as usize;
        let reset_counts = ChaindexingRepo::get_reset_counts(conn).await;
        let previous_reset_count = reset_counts.len();

        if reset_count > previous_reset_count {
            Self::reset_internal_migrations(client).await;
            Self::reset_migrations_for_contract_states(client, contracts).await;
            for _ in previous_reset_count..reset_count {
                ChaindexingRepo::create_reset_count(conn).await;
            }
        }
    }

    pub async fn run_migrations_for_resets(client: &ChaindexingRepoRawQueryClient) {
        ChaindexingRepo::migrate(
            &client,
            ChaindexingRepo::create_reset_counts_migration().to_vec(),
        )
        .await;
    }
    pub async fn run_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
        ChaindexingRepo::migrate(&client, ChaindexingRepo::get_internal_migrations()).await;
    }
    pub async fn reset_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
        ChaindexingRepo::migrate(&client, ChaindexingRepo::get_reset_internal_migrations()).await;
    }

    pub async fn run_migrations_for_contract_states(
        client: &ChaindexingRepoRawQueryClient,
        contracts: &Vec<Contract>,
    ) {
        for state_migration in Contracts::get_state_migrations(contracts) {
            ChaindexingRepo::migrate(client, state_migration.get_migrations()).await;
        }
    }
    pub async fn reset_migrations_for_contract_states(
        client: &ChaindexingRepoRawQueryClient,
        contracts: &Vec<Contract>,
    ) {
        for state_migration in Contracts::get_state_migrations(contracts) {
            ChaindexingRepo::migrate(client, state_migration.get_reset_migrations()).await;
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
