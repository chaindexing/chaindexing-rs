pub mod booting;
mod chain_reorg;
mod chains;
mod config;
mod contracts;
pub mod deferred_futures;
mod diesel;
pub mod events;
mod handlers;
pub mod ingester;
mod nodes;
mod pruning;
mod repos;
mod root;
pub mod states;

pub use chains::{Chain, ChainId};
pub use config::{Config, OptimizationConfig};
pub use contracts::{Contract, ContractAddress, ContractEvent, EventAbi, UnsavedContractAddress};
pub use events::{Event, EventParam};
pub use handlers::{
    PureHandler as EventHandler, PureHandlerContext as EventContext, SideEffectHandler,
    SideEffectHandlerContext as SideEffectContext,
};
pub use ingester::Provider as IngesterProvider;
pub use nodes::NodeHeartbeat as Heartbeat;
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
pub type ChaindexingRepoClient = PostgresRepoClient;

#[cfg(feature = "postgres")]
pub type ChaindexingRepoTxnClient<'a> = PostgresRepoTxnClient<'a>;

#[cfg(feature = "postgres")]
pub use repos::PostgresRepoAsyncConnection as ChaindexingRepoAsyncConnection;

pub use ethers::types::{Address, U256, U256 as BigInt, U256 as Uint};
use tokio::sync::Mutex;

pub type Bytes = Vec<u8>;

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

use config::ConfigError;
use nodes::NodeTasks;

use crate::nodes::{NodeTask, NodeTasksRunner};

pub type ChaindexingRepoClientMutex = Arc<Mutex<PostgresRepoClient>>;

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

pub async fn index_states<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Result<(), ChaindexingError> {
    config.validate()?;

    let client = config.repo.get_client().await;
    booting::setup_nodes(config, &client).await;
    let current_node = ChaindexingRepo::create_and_load_new_node(&client).await;
    wait_for_non_leader_nodes_to_abort(config.get_node_election_rate_ms()).await;

    booting::setup(config, &client).await?;

    let config = config.clone();
    tokio::spawn(async move {
        let mut interval =
            time::interval(Duration::from_millis(config.get_node_election_rate_ms()));

        let pool = config.repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;
        let conn = &mut conn;

        let mut node_tasks = NodeTasks::new(&current_node);

        loop {
            // Keep node active first to guarantee that at least this node is active before election
            ChaindexingRepo::keep_node_active(conn, &current_node).await;
            let active_nodes =
                ChaindexingRepo::get_active_nodes(conn, config.get_node_election_rate_ms()).await;

            node_tasks
                .orchestrate(
                    &config.optimization_config,
                    &active_nodes,
                    &get_tasks_runner(&config),
                )
                .await;

            interval.tick().await;
        }
    });

    Ok(())
}

pub async fn include_contract<'a, C: handlers::HandlerContext<'a>>(
    event_context: &C,
    contract_name: &str,
    address: &str,
) {
    let event = event_context.get_event();
    let chain_id = event.get_chain_id();
    let start_block_number = event.get_block_number();

    let contract_address =
        UnsavedContractAddress::new(contract_name, address, &chain_id, start_block_number);

    ChaindexingRepo::create_contract_address(event_context.get_client(), &contract_address).await;
}

async fn wait_for_non_leader_nodes_to_abort(node_election_rate_ms: u64) {
    time::sleep(Duration::from_millis(node_election_rate_ms)).await;
}

fn get_tasks_runner<S: Sync + Send + Debug + Clone + 'static>(
    config: &Config<S>,
) -> impl NodeTasksRunner + '_ {
    struct ChaindexingNodeTasksRunner<'a, S: Send + Sync + Clone + Debug + 'static> {
        config: &'a Config<S>,
    }
    #[async_trait::async_trait]
    impl<'a, S: Send + Sync + Clone + Debug + 'static> NodeTasksRunner
        for ChaindexingNodeTasksRunner<'a, S>
    {
        async fn run(&self) -> Vec<NodeTask> {
            let ingester = ingester::start(self.config).await;
            let handlers = handlers::start(self.config).await;

            vec![ingester, handlers]
        }
    }
    ChaindexingNodeTasksRunner { config }
}
