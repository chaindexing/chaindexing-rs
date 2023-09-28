use std::sync::Arc;

use derive_more::Display;
use futures_core::{future::BoxFuture, Stream};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::{
    contracts::{ContractAddressID, UnsavedContractAddress},
    events::Event,
    ContractAddress,
};

#[derive(Debug, Display)]
pub enum RepoError {}

#[async_trait::async_trait]
pub trait Repo:
    Sync + Send + Migratable + Streamable + ExecutesWithRawQuery + LoadsDataWithRawQuery + Clone
{
    type Pool;
    type Conn<'a>;

    fn new(url: &str) -> Self;
    async fn get_pool(&self, max_size: u32) -> Self::Pool;
    async fn get_conn<'a>(pool: &'a Self::Pool) -> Self::Conn<'a>;

    async fn run_in_transaction<'a, F>(conn: &mut Self::Conn<'a>, repo_ops: F)
    where
        F: for<'b> FnOnce(&'b mut Self::Conn<'a>) -> BoxFuture<'b, Result<(), ()>>
            + Send
            + Sync
            + 'a;

    async fn create_contract_addresses<'a>(
        conn: &mut Self::Conn<'a>,
        contract_addresses: &Vec<UnsavedContractAddress>,
    );
    async fn get_all_contract_addresses<'a>(conn: &mut Self::Conn<'a>) -> Vec<ContractAddress>;

    async fn create_events<'a>(conn: &mut Self::Conn<'a>, events: &Vec<Event>);
    async fn get_all_events<'a>(conn: &mut Self::Conn<'a>) -> Vec<Event>;

    async fn update_next_block_number_to_ingest_from<'a>(
        conn: &mut Self::Conn<'a>,
        contract_address: &ContractAddress,
        block_number: i64,
    );
    async fn update_next_block_number_to_handle_from<'a>(
        conn: &mut Self::Conn<'a>,
        contract_address_id: ContractAddressID,
        block_number: i64,
    );
}

#[async_trait::async_trait]
pub trait HasRawQueryClient {
    type RawQueryClient: Send + Sync;

    async fn get_raw_query_client(&self) -> Self::RawQueryClient;
}

#[async_trait::async_trait]
pub trait ExecutesWithRawQuery: HasRawQueryClient {
    async fn execute_raw_query(client: &Self::RawQueryClient, query: &str);
}

#[async_trait::async_trait]
pub trait LoadsDataWithRawQuery: HasRawQueryClient {
    async fn load_data_list_from_raw_query<Data: Send + DeserializeOwned>(
        conn: &Self::RawQueryClient,
        query: &str,
    ) -> Vec<Data>;
}

pub trait Streamable {
    type StreamConn<'a>;
    fn get_contract_addresses_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a>;
    fn get_events_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
        from: i64,
    ) -> Box<dyn Stream<Item = Vec<Event>> + Send + Unpin + 'a>;
}

pub trait RepoMigrations: Migratable {
    fn create_contract_addresses_migration() -> &'static [&'static str];
    fn create_events_migration() -> &'static [&'static str];

    fn get_all_internal_migrations() -> Vec<&'static str> {
        [
            Self::create_contract_addresses_migration(),
            Self::create_events_migration(),
        ]
        .concat()
    }
}

#[async_trait::async_trait]
pub trait Migratable: ExecutesWithRawQuery + Sync + Send {
    async fn migrate(client: &Self::RawQueryClient, migrations: Vec<&'static str>)
    where
        Self: Sized,
    {
        for migration in migrations {
            Self::execute_raw_query(client, migration).await;
        }
    }
}

pub struct SQLikeMigrations;

impl SQLikeMigrations {
    pub fn create_contract_addresses() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_contract_addresses (
                id  SERIAL PRIMARY KEY,
                address TEXT  NOT NULL,
                contract_name TEXT NOT NULL,
                chain_id INTEGER NOT NULL,
                start_block_number BIGINT NOT NULL,
                next_block_number_to_ingest_from BIGINT NULL,
                next_block_number_to_handle_from BIGINT NULL
        )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_contract_addresses_address_index
        ON chaindexing_contract_addresses(address)",
        ]
    }

    pub fn create_events() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_events (
                id uuid PRIMARY KEY,
                contract_address TEXT NOT NULL,
                contract_name TEXT NOT NULL,
                abi TEXT NOT NULL,
                log_params JSON NOT NULL,
                parameters JSON NOT NULL,
                topics JSON NOT NULL,
                block_hash TEXT NOT NULL,
                block_number BIGINT NOT NULL,
                transaction_hash TEXT NOT NULL,
                transaction_index BIGINT NOT NULL,
                log_index BIGINT NOT NULL,
                removed BOOLEAN NOT NULL,
                inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_events_transaction_hash_log_index
            ON chaindexing_events(transaction_hash,log_index)",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_abi
            ON chaindexing_events(abi)",
        ]
    }
}
