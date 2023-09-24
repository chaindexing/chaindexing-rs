use std::sync::Arc;

use crate::{
    contracts::{ContractAddress, ContractAddressID, UnsavedContractAddress},
    events::Event,
    Streamable,
};
use diesel_async::RunQueryDsl;

use diesel::{result::Error, sql_query, upsert::excluded, ExpressionMethods};
use diesel_async::{pooled_connection::AsyncDieselConnectionManager, AsyncPgConnection};
use diesel_streamer::get_serial_table_async_stream;
use futures_core::{future::BoxFuture, Stream};
use tokio::sync::Mutex;

use super::repo::{ExecutesRawQuery, Migratable, Migration, Repo, SQLikeMigrations};

pub type Conn<'a> = bb8::PooledConnection<'a, AsyncDieselConnectionManager<AsyncPgConnection>>;
pub type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

pub use diesel_async::{
    scoped_futures::ScopedFutureExt as PostgresRepoTransactionExt,
    AsyncConnection as PostgresRepoAsyncConnection,
};

#[derive(Clone)]
pub struct PostgresRepo {
    url: String,
}

type PgPooledConn<'a> = bb8::PooledConnection<'a, AsyncDieselConnectionManager<AsyncPgConnection>>;

#[async_trait::async_trait]
impl Repo for PostgresRepo {
    type Conn<'a> = PgPooledConn<'a>;
    type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    async fn get_pool(&self, max_size: u32) -> Pool {
        let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&self.url);

        bb8::Pool::builder().max_size(max_size).build(manager).await.unwrap()
    }

    async fn get_conn<'a>(pool: &'a Pool) -> Conn<'a> {
        pool.get().await.unwrap()
    }

    async fn run_in_transaction<'a, F>(conn: &mut Conn<'a>, repo_ops: F)
    where
        F: for<'b> FnOnce(&'b mut Conn<'a>) -> BoxFuture<'b, Result<(), ()>> + Send + Sync + 'a,
    {
        conn.transaction::<(), Error, _>(|transaction_conn| {
            async move {
                (repo_ops)(transaction_conn).await.unwrap();

                Ok(())
            }
            .scope_boxed()
        })
        .await
        .unwrap();
    }

    async fn create_contract_addresses<'a>(
        conn: &mut Conn<'a>,
        contract_addresses: &Vec<UnsavedContractAddress>,
    ) {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::{
            address, chaindexing_contract_addresses,
        };

        diesel::insert_into(chaindexing_contract_addresses)
            .values(contract_addresses)
            .on_conflict(address)
            .do_update()
            .set(address.eq(excluded(address)))
            .execute(conn)
            .await
            .unwrap();
    }

    async fn get_all_contract_addresses<'a>(conn: &mut Conn<'a>) -> Vec<ContractAddress> {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        chaindexing_contract_addresses.load(conn).await.unwrap()
    }

    async fn create_events<'a>(conn: &mut Conn<'a>, events: &Vec<Event>) {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        diesel::insert_into(chaindexing_events)
            .values(events)
            .execute(conn)
            .await
            .unwrap();
    }

    async fn get_all_events<'a>(conn: &mut Conn<'a>) -> Vec<Event> {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        chaindexing_events.load(conn).await.unwrap()
    }

    async fn update_next_block_number_to_ingest_from<'a>(
        conn: &mut Self::Conn<'a>,
        contract_address: &ContractAddress,
        block_number: i64,
    ) {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        diesel::update(chaindexing_contract_addresses)
            .filter(id.eq(contract_address.id))
            .set(next_block_number_to_ingest_from.eq(block_number))
            .execute(conn)
            .await
            .unwrap();
    }

    async fn update_next_block_number_to_handle_from<'a>(
        conn: &mut Conn<'a>,
        ContractAddressID(contract_address_id): ContractAddressID,
        block_number: i64,
    ) {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        diesel::update(chaindexing_contract_addresses)
            .filter(id.eq(contract_address_id))
            .set(next_block_number_to_handle_from.eq(block_number))
            .execute(conn)
            .await
            .unwrap();
    }
}

impl Streamable for PostgresRepo {
    type StreamConn<'a> = PgPooledConn<'a>;

    fn get_contract_addresses_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a> {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        get_serial_table_async_stream!(
            chaindexing_contract_addresses,
            id,
            conn,
            Arc<Mutex<PgPooledConn<'a>>>,
            ContractAddress,
            i32
        )
    }

    fn get_events_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
        from: i64,
    ) -> Box<dyn Stream<Item = Vec<Event>> + Send + Unpin + 'a> {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        const CHUNK_SIZE: i32 = 500;

        get_serial_table_async_stream!(
            chaindexing_events,
            block_number,
            conn,
            Arc<Mutex<PgPooledConn<'a>>>,
            Event,
            i64,
            CHUNK_SIZE,
            Some(from)
        )
    }
}

#[async_trait::async_trait]
impl ExecutesRawQuery for PostgresRepo {
    type RawQueryConn<'a> = PgPooledConn<'a>;

    async fn execute_raw_query<'a>(&self, conn: &mut Conn<'a>, query: &str) {
        sql_query(query).execute(conn).await.unwrap();
    }
}

impl Migratable for PostgresRepo {
    fn create_contract_addresses_migration() -> Vec<Migration> {
        SQLikeMigrations::create_contract_addresses()
    }

    fn create_events_migration() -> Vec<Migration> {
        SQLikeMigrations::create_events()
    }
}
