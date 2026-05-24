// TODO: Add back
// #![warn(
//     missing_debug_implementations,
//     missing_docs,
//     rust_2018_idioms,
//     unreachable_pub
// )]

//! # Chaindexing
//! Index any EVM chain and query in SQL.
//!
//! View working examples here: <https://github.com/chaindexing/chaindexing-examples/tree/main/rust>.
mod chain_blocks;
mod chain_reorg;
mod chains;
mod checkpoints;
mod config;
mod contracts;
mod diesel;
mod handlers;
mod indexer;
mod nodes;
mod outbox;
mod pruning;
mod repos;
mod root;

/// Augmenting modules for standard library to support Chaindexing's operations
pub mod augmenting_std;

pub use chain_reorg::{IndexingFinality, ReorgMode, SideEffectFinality};
pub use chains::{Chain, ChainId};
pub use config::{Config, OptimizationConfig};
pub use contracts::{Contract, ContractAddress, EventAbi};
pub use events::{Event, EventParam};
pub use handlers::{
    PureHandler as EventHandler, PureHandlerContext as EventContext, SideEffectHandler,
    SideEffectHandlerContext as SideEffectContext,
};
pub use indexer::Indexer;
pub use nodes::NodeHeartbeat as Heartbeat;
pub use outbox::{
    dispatch_pending_outbox_jobs, OutboxDispatchConfig, OutboxDispatcher, OutboxFinalityWatermark,
    OutboxJob, OutboxReceipt,
};

pub use chaindexing_macros::state_migrations;
pub use ethers::types::{I256, U256};
use tokio::sync::Mutex;

/// Houses traits and structs for implementing states that can be indexed.
pub mod states;

/// Hexadecimal representation of addresses (such as contract addresses)
pub type Address = ethers::types::Address;
/// Represents bytes
pub type Bytes = Vec<u8>;
#[cfg(feature = "postgres")]
pub use repos::PostgresRepo;

#[doc(hidden)]
pub mod booting;
#[doc(hidden)]
pub mod deferred_futures;
#[doc(hidden)]
pub mod events;
#[doc(hidden)]
pub mod ingester;
#[doc(hidden)]
pub use contracts::{ContractEvent, UnsavedContractAddress};
#[doc(hidden)]
pub use ingester::Provider as IngesterProvider;
#[doc(hidden)]
pub use repos::*;

#[doc(hidden)]
#[cfg(feature = "postgres")]
pub use repos::{PostgresRepoConn, PostgresRepoPool};

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub type ChaindexingRepo = PostgresRepo;

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub type ChaindexingRepoPool = PostgresRepoPool;

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub type ChaindexingRepoConn<'a> = PostgresRepoConn<'a>;

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub type ChaindexingRepoClient = PostgresRepoClient;

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub type ChaindexingRepoTxnClient<'a> = PostgresRepoTxnClient<'a>;

#[cfg(feature = "postgres")]
#[doc(hidden)]
pub use repos::PostgresRepoAsyncConnection as ChaindexingRepoAsyncConnection;

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinError;
use tokio::time;

use config::ConfigError;
use nodes::NodeTasks;

use crate::nodes::{NodeTask, NodeTasksRunner};

pub(crate) type ChaindexingRepoClientMutex = Arc<Mutex<PostgresRepoClient>>;

/// Errors from mis-configurations, database connections, internal errors, etc.
#[derive(Debug)]
pub enum ChaindexingError {
    Config(ConfigError),
    Runtime(String),
}

impl From<ConfigError> for ChaindexingError {
    fn from(value: ConfigError) -> Self {
        ChaindexingError::Config(value)
    }
}

impl std::fmt::Display for ChaindexingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChaindexingError::Config(config_error) => {
                write!(f, "Config error: {config_error}")
            }
            ChaindexingError::Runtime(error) => {
                write!(f, "Runtime error: {error}")
            }
        }
    }
}

impl std::error::Error for ChaindexingError {}

/// Handle for a running Chaindexing indexer.
///
/// Use this when embedding Chaindexing in a larger service that owns shutdown.
pub struct IndexingHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl IndexingHandle {
    /// Requests cooperative shutdown for the indexer and its workers.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Waits until the indexer task exits.
    pub async fn wait(self) -> Result<(), ChaindexingError> {
        supervisor_result(self.join_handle.await)
    }

    /// Runs until Ctrl-C is received or the supervisor exits unexpectedly.
    pub async fn wait_for_shutdown_signal(mut self) -> Result<(), ChaindexingError> {
        tokio::select! {
            result = &mut self.join_handle => {
                supervisor_result(result)?;
                Err(ChaindexingError::Runtime(
                    "indexer supervisor exited before a shutdown signal".to_string()
                ))
            },
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| ChaindexingError::Runtime(error.to_string()))?;
                self.shutdown();
                supervisor_result(self.join_handle.await)
            }
        }
    }
}

/// Starts processes for ingesting events and indexing states as configured.
pub async fn index_states<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Result<(), ChaindexingError> {
    start_indexing(config).await?.wait_for_shutdown_signal().await
}

/// Starts indexing workers and returns a handle for lifecycle management.
pub async fn start_indexing<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Result<IndexingHandle, ChaindexingError> {
    config.validate()?;

    let client = config.repo.get_client().await;
    booting::setup_nodes(config, &client).await;
    let current_node = ChaindexingRepo::create_and_load_new_node(&client).await;
    wait_for_non_leader_nodes_to_abort(config.get_node_election_rate_ms()).await;

    booting::setup(config, &client).await?;

    let config = config.clone();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let join_handle = tokio::spawn(async move {
        let mut interval =
            time::interval(Duration::from_millis(config.get_node_election_rate_ms()));

        let pool = config.repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;
        let conn = &mut conn;

        let mut node_tasks = NodeTasks::new(&current_node);

        loop {
            if *shutdown_rx.borrow() {
                break;
            }

            // Keep node active first to guarantee that at least this node is active before election
            ChaindexingRepo::keep_node_active(conn, &current_node).await;
            let is_leader = ChaindexingRepo::try_advisory_lock(conn, config.leader_lock_id).await;

            node_tasks
                .orchestrate_with_leadership(
                    is_leader,
                    &config.optimization_config,
                    &get_tasks_runner(&config),
                )
                .await;

            tokio::select! {
                _ = interval.tick() => {}
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }

        node_tasks.stop().await;
    });

    Ok(IndexingHandle {
        shutdown_tx,
        join_handle,
    })
}

/// Includes runtime-discovered contract addresses for indexing.
///
/// # Arguments
///
/// * `event_context` - context where the contract was discovered.
///   N/B: Indexing for this contract starts from this point onwards
/// * `name` -  name of the contract as defined in the config
/// * `address` -  address of discovered contract
///
/// # Example
///
/// ```ignore
/// // In an EventHandler...
/// chaindexing::include_contract(&context, "UniswapV3Pool", &pool_contract_address)
///  .await;
/// // Includes a new UniswapV3Pool contract:{pool_contract_address} for indexing...
/// ```
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
    #[crate::augmenting_std::async_trait]
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

fn supervisor_result(result: Result<(), JoinError>) -> Result<(), ChaindexingError> {
    result.map_err(|error| ChaindexingError::Runtime(error.to_string()))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[tokio::test]
    async fn wait_for_shutdown_signal_errors_when_supervisor_exits_early() {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let handle = IndexingHandle {
            shutdown_tx,
            join_handle: tokio::spawn(async {}),
        };

        let error = handle.wait_for_shutdown_signal().await.unwrap_err();

        assert!(error.to_string().contains("indexer supervisor exited before a shutdown signal"));
    }
}

pub mod prelude {
    pub use crate::augmenting_std::{async_trait, serde};
    pub use crate::chains::{Chain, ChainId};
    pub use crate::config::{Config, OptimizationConfig};
    pub use crate::contracts::{Contract, ContractAddress, EventAbi};
    pub use crate::events::{Event, EventParam};
    pub use crate::handlers::{
        PureHandler as EventHandler, PureHandlerContext as EventContext, SideEffectHandler,
        SideEffectHandlerContext as SideEffectContext,
    };
    pub use crate::indexer::Indexer;
    pub use crate::nodes::NodeHeartbeat as Heartbeat;
    pub use crate::outbox::{
        dispatch_pending_outbox_jobs, OutboxDispatchConfig, OutboxDispatcher, OutboxJob,
        OutboxReceipt,
    };
    pub use crate::states::{
        ChainState, ContractState, Filters, MultiChainState, StateMigrations, Updates,
    };
    pub use crate::Address;
    pub use crate::IndexingHandle;
    pub use chaindexing_macros::state_migrations;
    pub use ethers::types::{I256, U256};
}
