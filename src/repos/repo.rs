use derive_more::Display;
use futures_core::future::BoxFuture;
use futures_util::{stream, StreamExt};

use crate::{contracts::UnsavedContractAddress, events::Event, ContractAddress};

#[derive(Debug, Display)]
pub enum RepoError {}

#[async_trait::async_trait]
pub trait Repo<Conn>: Sync + Send + Migratable + Clone {
    async fn new(url: &str) -> Self;
    async fn create_contract_addresses(&self, contract_addresses: &Vec<UnsavedContractAddress>);
    async fn stream_contract_addresses<'a, F>(&self, _processor: F)
    where
        F: Fn(Vec<ContractAddress>) -> BoxFuture<'a, ()> + Sync + std::marker::Send;
    async fn create_events_and_update_last_ingested_block_number(
        &self,
        events: &Vec<Event>,
        contract_addresses: &Vec<ContractAddress>,
        block_number: i32,
    );
}

pub type Migration = &'static str;
pub type MigrationList = Vec<Migration>;

#[async_trait::async_trait]
pub trait Migratable {
    fn create_contract_addresses_migration() -> MigrationList;
    fn create_events_migration() -> MigrationList;

    async fn migrate<'a, F>(raw_query_executor: F)
    where
        F: Fn(Migration) -> BoxFuture<'a, ()> + Sync + std::marker::Send,
    {
        stream::iter(
            Self::create_contract_addresses_migration()
                .into_iter()
                .chain(Self::create_events_migration()),
        )
        .for_each(|migration| async {
            (raw_query_executor)(migration).await;
        })
        .await;
    }
}

pub struct SQLikeMigrations;

impl SQLikeMigrations {
    pub fn create_contract_addresses() -> MigrationList {
        vec![
            "CREATE TABLE IF NOT EXISTS chaindexing_contract_addresses (
                id  SERIAL PRIMARY KEY,
                address TEXT  NOT NULL,
                contract_name TEXT NOT NULL,
                chain_id INTEGER NOT NULL,
                start_block_number INTEGER NOT NULL,
                last_ingested_block_number INTEGER NULL
        )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_contract_addresses_address_index
        ON chaindexing_contract_addresses(address)",
        ]
    }

    pub fn create_events() -> MigrationList {
        vec![
            "CREATE TABLE IF NOT EXISTS chaindexing_events (
                id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
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
