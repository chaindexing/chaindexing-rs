use derive_more::Display;
use std::fmt::Debug;
use std::sync::Arc;
use uuid::Uuid;

use futures_core::{future::BoxFuture, Stream};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::chain_reorg::{ReorgedBlock, UnsavedReorgedBlock};
use crate::handler_subscriptions::HandlerSubscription;
use crate::root;
use crate::{
    contracts::UnsavedContractAddress,
    events::{Event, PartialEvent},
    nodes::Node,
    ContractAddress,
};

#[derive(Debug, Display)]
pub enum RepoError {
    NotConnected,
    Unknown(String),
}

#[async_trait::async_trait]
pub trait Repo:
    Sync + Send + Migratable + Streamable + ExecutesWithRawQuery + LoadsDataWithRawQuery + Clone + Debug
{
    type Pool;
    type Conn<'a>;

    async fn get_pool(&self, max_size: u32) -> Self::Pool;
    async fn get_conn<'a>(pool: &'a Self::Pool) -> Self::Conn<'a>;

    async fn run_in_transaction<'a, F>(
        conn: &mut Self::Conn<'a>,
        repo_ops: F,
    ) -> Result<(), RepoError>
    where
        F: for<'b> FnOnce(&'b mut Self::Conn<'a>) -> BoxFuture<'b, Result<(), RepoError>>
            + Send
            + Sync
            + 'a;

    /// Upserts contract addresses and updates their contract names when there is a conflict
    async fn upsert_contract_addresses<'a>(
        conn: &mut Self::Conn<'a>,
        contract_addresses: &[UnsavedContractAddress],
    );
    async fn get_all_contract_addresses<'a>(conn: &mut Self::Conn<'a>) -> Vec<ContractAddress>;

    async fn create_events<'a>(conn: &mut Self::Conn<'a>, events: &[Event]);
    async fn get_all_events<'a>(conn: &mut Self::Conn<'a>) -> Vec<Event>;
    async fn get_events<'a>(
        conn: &mut Self::Conn<'a>,
        address: String,
        from: u64,
        to: u64,
    ) -> Vec<Event>;
    async fn delete_events_by_ids<'a>(conn: &mut Self::Conn<'a>, ids: &[Uuid]);

    async fn update_next_block_number_to_ingest_from<'a>(
        conn: &mut Self::Conn<'a>,
        contract_address: &ContractAddress,
        block_number: i64,
    );

    async fn create_reorged_block<'a>(
        conn: &mut Self::Conn<'a>,
        reorged_block: &UnsavedReorgedBlock,
    );

    async fn create_node<'a>(conn: &mut Self::Conn<'a>) -> Node;
    async fn get_active_nodes<'a>(
        conn: &mut Self::Conn<'a>,
        node_election_rate_ms: u64,
    ) -> Vec<Node>;
    async fn keep_node_active<'a>(conn: &mut Self::Conn<'a>, node: &Node);
}

#[async_trait::async_trait]
pub trait HasRawQueryClient {
    type RawQueryClient: Send + Sync;
    type RawQueryTxnClient<'a>: Send + Sync;

    async fn get_raw_query_client(&self) -> Self::RawQueryClient;
    async fn get_raw_query_txn_client<'a>(
        client: &'a mut Self::RawQueryClient,
    ) -> Self::RawQueryTxnClient<'a>;
}

#[async_trait::async_trait]
pub trait ExecutesWithRawQuery: HasRawQueryClient {
    async fn execute_raw_query(client: &Self::RawQueryClient, query: &str);
    async fn execute_raw_query_in_txn<'a>(client: &Self::RawQueryTxnClient<'a>, query: &str);
    async fn commit_raw_query_txns<'a>(client: Self::RawQueryTxnClient<'a>);

    async fn create_handler_subscriptions(
        client: &Self::RawQueryClient,
        handler_subscriptions: &[HandlerSubscription],
    );

    async fn create_contract_address<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        contract_address: &UnsavedContractAddress,
    );

    async fn update_handler_subscription_next_block_number_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_id: u64,
        block_number: u64,
    );

    async fn update_handler_subscription_next_block_number_for_side_effect<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_id: u64,
        block_number: u64,
    );

    async fn update_reorged_blocks_as_handled<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        reorged_block_ids: &[i32],
    );

    async fn append_root_state(client: &Self::RawQueryClient, new_root_state: &root::State);
    async fn prune_events(client: &Self::RawQueryClient, min_block_number: u64, chain_id: u64);
    async fn prune_nodes(client: &Self::RawQueryClient, retain_size: u16);
    async fn prune_root_states(client: &Self::RawQueryClient, retain_size: u64);
}

#[async_trait::async_trait]
pub trait LoadsDataWithRawQuery: HasRawQueryClient {
    async fn load_last_root_state(client: &Self::RawQueryClient) -> Option<root::State>;
    async fn load_latest_events(
        client: &Self::RawQueryClient,
        addresses: &[String],
    ) -> Vec<PartialEvent>;
    async fn load_unhandled_reorged_blocks(client: &Self::RawQueryClient) -> Vec<ReorgedBlock>;
    async fn load_handler_subscriptions(
        client: &Self::RawQueryClient,
        chain_ids: &[u64],
    ) -> Vec<HandlerSubscription>;
    async fn load_events(
        client: &Self::RawQueryClient,
        chain_ids: &[u64],
        from_block_number: u64,
        to_block_number: u64,
    ) -> Vec<Event>;

    async fn load_data_from_raw_query<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Option<Data>;
    async fn load_data_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
        client: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Option<Data>;
    async fn load_data_list_from_raw_query<Data: Send + DeserializeOwned>(
        conn: &Self::RawQueryClient,
        query: &str,
    ) -> Vec<Data>;
    async fn load_data_list_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
        conn: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Vec<Data>;
}

pub trait Streamable {
    type StreamConn<'a>;
    fn get_contract_addresses_stream_by_chain<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
        chain_id_: i64,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a>;
}

pub trait RepoMigrations: Migratable {
    fn create_root_states_migration() -> &'static [&'static str];

    fn create_nodes_migration() -> &'static [&'static str];
    fn create_contract_addresses_migration() -> &'static [&'static str];
    fn drop_contract_addresses_migration() -> &'static [&'static str];
    fn create_events_migration() -> &'static [&'static str];
    fn drop_events_migration() -> &'static [&'static str];
    fn create_reorged_blocks_migration() -> &'static [&'static str];
    fn drop_reorged_blocks_migration() -> &'static [&'static str];
    fn create_handler_subscriptions_migration() -> &'static [&'static str];
    fn nullify_handler_subscriptions_next_block_number_to_handle_from_migration(
    ) -> &'static [&'static str];
    fn drop_handler_subscriptions_migration() -> &'static [&'static str];

    fn get_internal_migrations() -> Vec<&'static str> {
        [
            Self::create_contract_addresses_migration(),
            Self::create_events_migration(),
            Self::create_reorged_blocks_migration(),
            Self::create_handler_subscriptions_migration(),
        ]
        .concat()
    }

    fn get_reset_internal_migrations() -> Vec<&'static str> {
        [
            Self::drop_contract_addresses_migration(),
            Self::drop_events_migration(),
            Self::drop_reorged_blocks_migration(),
            Self::nullify_handler_subscriptions_next_block_number_to_handle_from_migration(),
        ]
        .concat()
    }
}

#[async_trait::async_trait]
pub trait Migratable: ExecutesWithRawQuery + Sync + Send {
    async fn migrate(client: &Self::RawQueryClient, migrations: Vec<impl AsRef<str> + Send + Sync>)
    where
        Self: Sized,
    {
        for migration in migrations {
            Self::execute_raw_query(client, migration.as_ref()).await;
        }
    }
}

pub struct SQLikeMigrations;

impl SQLikeMigrations {
    pub fn create_root_states() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_root_states (
                id BIGSERIAL PRIMARY KEY,
                reset_count BIGINT NOT NULL,
                reset_including_side_effects_count BIGINT NOT NULL
            )"]
    }

    pub fn create_nodes() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_nodes (
                id SERIAL PRIMARY KEY,
                last_active_at BIGINT DEFAULT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT,
                inserted_at BIGINT DEFAULT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT
        )"]
    }

    pub fn create_contract_addresses() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_contract_addresses (
                id SERIAL PRIMARY KEY,
                address VARCHAR NOT NULL,
                contract_name VARCHAR NOT NULL,
                chain_id BIGINT NOT NULL,
                start_block_number BIGINT NOT NULL,
                next_block_number_to_ingest_from BIGINT NOT NULL
        )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_contract_addresses_chain_address_index
        ON chaindexing_contract_addresses(chain_id, address)",
        ]
    }
    pub fn drop_contract_addresses() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_contract_addresses"]
    }

    pub fn create_events() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_events (
                id uuid PRIMARY KEY,
                chain_id BIGINT NOT NULL,
                contract_address VARCHAR NOT NULL,
                contract_name VARCHAR NOT NULL,
                abi TEXT NOT NULL,
                parameters JSON NOT NULL,
                topics JSON NOT NULL,
                block_hash VARCHAR NOT NULL,
                block_number BIGINT NOT NULL,
                block_timestamp BIGINT NOT NULL,
                transaction_hash VARCHAR NOT NULL,
                transaction_index INTEGER NOT NULL,
                log_index INTEGER NOT NULL,
                removed BOOLEAN NOT NULL,
                inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
            )",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_chain_contract_block_log_index
            ON chaindexing_events(chain_id,contract_address,block_number,log_index)",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_abi
            ON chaindexing_events(abi)",
        ]
    }
    pub fn drop_events() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_events"]
    }

    pub fn create_reorged_blocks() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_reorged_blocks (
                id SERIAL PRIMARY KEY,
                chain_id BIGINT NOT NULL,
                block_number BIGINT NOT NULL,
                handled_at BIGINT,
                inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
            )"]
    }
    pub fn drop_reorged_blocks() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_reorged_blocks"]
    }

    pub fn create_handler_subscriptions() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_handler_subscriptions (
                id SERIAL PRIMARY KEY,
                chain_id BIGINT NOT NULL,
                next_block_number_to_handle_from BIGINT NOT NULL,
                next_block_number_for_side_effects BIGINT DEFAULT 0
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_handler_subscriptions_chain
        ON chaindexing_handler_subscriptions(chain_id)",
        ]
    }

    pub fn nullify_handler_subscriptions_next_block_number_to_handle_from(
    ) -> &'static [&'static str] {
        &["UPDATE TABLE chaindexing_handler_subscriptions SET next_block_number_to_handle_from = 0"]
    }

    pub fn drop_handler_subscriptions() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_handler_subscriptions"]
    }
}
