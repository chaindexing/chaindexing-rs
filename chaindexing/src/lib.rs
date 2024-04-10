mod chain_reorg;
mod chains;
mod config;
pub mod contract_states;
mod contracts;
mod diesel;
mod event_handlers;
pub mod events;
pub mod events_ingester;
mod nodes;
mod pruning;
mod repos;
mod reset_counts;

pub use chains::{Chain, ChainId};
pub use config::{Config, OptimizationConfig};
pub use contracts::{Contract, ContractAddress, ContractEvent, Contracts, UnsavedContractAddress};
pub use event_handlers::{EventHandler, EventHandlerContext as EventContext, EventHandlers};
pub use events::{Event, EventParam};
pub use events_ingester::Provider as EventsIngesterProvider;
pub use nodes::KeepNodeActiveRequest;
pub use repos::*;

use config::ConfigError;
use nodes::NodeTasks;
use std::fmt::Debug;
use std::time::Duration;

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
use tokio::time;

pub use ethers::types::{Address, U256, U256 as BigInt, U256 as Uint};
pub type Bytes = Vec<u8>;

pub enum ChaindexingError {
    Config(ConfigError),
}

impl From<ConfigError> for ChaindexingError {
    fn from(value: ConfigError) -> Self {
        ChaindexingError::Config(value)
    }
}

impl Debug for ChaindexingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChaindexingError::Config(config_error) => {
                write!(f, "Config Error: {:?}", config_error)
            }
        }
    }
}

pub async fn include_contract_in_indexing<'a, S: Send + Sync + Clone>(
    event_context: &EventContext<'a, S>,
    contract_name: &str,
    address: &str,
) {
    let chain_id = event_context.event.get_chain_id();
    let start_block_number = event_context.event.get_block_number();

    let contract_address =
        UnsavedContractAddress::new(contract_name, address, &chain_id, start_block_number);

    ChaindexingRepo::create_contract_address(event_context.raw_query_client, &contract_address)
        .await;
}

pub async fn index_states<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Result<(), ChaindexingError> {
    config.validate()?;

    let Config { repo, .. } = config;
    let query_client = repo.get_raw_query_client().await;
    let pool = repo.get_pool(1).await;
    let mut conn = ChaindexingRepo::get_conn(&pool).await;

    ChaindexingRepo::migrate(
        &query_client,
        ChaindexingRepo::create_nodes_migration().to_vec(),
    )
    .await;
    ChaindexingRepo::prune_nodes(&query_client, config.max_concurrent_node_count).await;
    let current_node = ChaindexingRepo::create_node(&mut conn).await;

    wait_for_non_leader_nodes_to_abort(config.get_node_election_rate_ms()).await;

    setup(config, &mut conn, &query_client).await?;

    let config = config.clone();
    tokio::spawn(async move {
        let mut interval =
            time::interval(Duration::from_millis(config.get_node_election_rate_ms()));

        let pool = config.repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;

        let mut node_tasks = NodeTasks::new(&current_node);

        loop {
            node_tasks.orchestrate(&config, &mut conn).await;

            interval.tick().await;
        }
    });

    Ok(())
}
async fn wait_for_non_leader_nodes_to_abort(node_election_rate_ms: u64) {
    time::sleep(Duration::from_millis(node_election_rate_ms)).await;
}
pub async fn setup<'a, S: Sync + Send + Clone>(
    config: &Config<S>,
    conn: &mut ChaindexingRepoConn<'a>,
    client: &ChaindexingRepoRawQueryClient,
) -> Result<(), ChaindexingError> {
    let Config {
        contracts,
        reset_count,
        reset_queries,
        ..
    } = config;

    run_migrations_for_resets(client).await;
    maybe_reset(reset_count, reset_queries, contracts, client, conn).await;
    run_internal_migrations(client).await;
    run_migrations_for_contract_states(client, contracts).await;

    let contract_addresses: Vec<_> =
        contracts.clone().into_iter().flat_map(|c| c.addresses).collect();
    ChaindexingRepo::create_contract_addresses(conn, &contract_addresses).await;

    Ok(())
}
pub async fn maybe_reset<'a, S: Send + Sync + Clone>(
    reset_count: &u64,
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoRawQueryClient,
    conn: &mut ChaindexingRepoConn<'a>,
) {
    let reset_count = *reset_count;
    let previous_reset_count_id = ChaindexingRepo::get_last_reset_count(conn)
        .await
        .map(|rc| rc.get_count())
        .unwrap_or(0);

    if reset_count > previous_reset_count_id {
        reset_internal_migrations(client).await;
        reset_migrations_for_contract_states(client, contracts).await;
        run_user_reset_queries(client, reset_queries).await;
        for _ in previous_reset_count_id..reset_count {
            ChaindexingRepo::create_reset_count(conn).await;
        }
    }
}

pub async fn run_migrations_for_resets(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_reset_counts_migration().to_vec(),
    )
    .await;
    ChaindexingRepo::prune_reset_counts(client, reset_counts::MAX_RESET_COUNT).await;
}
pub async fn run_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_internal_migrations()).await;
}
pub async fn reset_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_reset_internal_migrations()).await;
}

pub async fn run_migrations_for_contract_states<S: Send + Sync + Clone>(
    client: &ChaindexingRepoRawQueryClient,
    contracts: &[Contract<S>],
) {
    for state_migration in Contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_migrations()).await;
    }
}
pub async fn reset_migrations_for_contract_states<S: Send + Sync + Clone>(
    client: &ChaindexingRepoRawQueryClient,
    contracts: &[Contract<S>],
) {
    for state_migration in Contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_reset_migrations()).await;
    }
}

async fn run_user_reset_queries(
    client: &ChaindexingRepoRawQueryClient,
    reset_queries: &Vec<String>,
) {
    for reset_query in reset_queries {
        ChaindexingRepo::execute_raw_query(client, reset_query).await;
    }
}
