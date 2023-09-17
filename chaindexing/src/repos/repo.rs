use std::sync::Arc;

use derive_more::Display;
use futures_core::{future::BoxFuture, Stream};
use futures_util::{stream, StreamExt};
use tokio::sync::Mutex;

use crate::{contracts::UnsavedContractAddress, events::Event, ContractAddress};

#[derive(Debug, Display)]
pub enum RepoError {}

#[async_trait::async_trait]
pub trait Repo: Sync + Send + Migratable + Clone {
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

    async fn get_contract_addresses_streamer<'a>(
        conn: Arc<Mutex<Self::Conn<'a>>>,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a>;

    async fn create_events<'a>(conn: &mut Self::Conn<'a>, events: &Vec<Event>);
    async fn get_all_events<'a>(conn: &mut Self::Conn<'a>) -> Vec<Event>;

    async fn update_last_ingested_block_number<'a>(
        conn: &mut Self::Conn<'a>,
        contract_addresses: &Vec<ContractAddress>,
        block_number: i32,
    );
}

pub type Migration = &'static str;

#[async_trait::async_trait]
pub trait ExecutesRawQuery {
    type RawQueryConn<'a>: Send;
    async fn execute_raw_query<'a>(&self, conn: &mut Self::RawQueryConn<'a>, query: &str);
}

#[async_trait::async_trait]
pub trait Migratable: ExecutesRawQuery + Sync + Send {
    fn create_contract_addresses_migration() -> Vec<Migration>;
    fn create_events_migration() -> Vec<Migration>;

    async fn migrate<'a>(&self, conn: &mut Self::RawQueryConn<'a>) {
        let conn = Arc::new(Mutex::new(conn));

        stream::iter(
            Self::create_contract_addresses_migration()
                .into_iter()
                .chain(Self::create_events_migration()),
        )
        .for_each(|migration| async {
            let conn = conn.clone();
            let mut conn = conn.lock().await;

            self.execute_raw_query(&mut conn, migration).await;
        })
        .await;
    }
}

pub struct SQLikeMigrations;

impl SQLikeMigrations {
    pub fn create_contract_addresses() -> Vec<Migration> {
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

    pub fn create_events() -> Vec<Migration> {
        vec![
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