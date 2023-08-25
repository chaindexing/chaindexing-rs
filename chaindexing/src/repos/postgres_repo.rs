use crate::{
    contracts::{ContractAddress, UnsavedContractAddress},
    events::Event,
};
use diesel_async::RunQueryDsl;
use diesel_streamer::stream_serial_table;

use diesel::{result::Error, sql_query, upsert::excluded, ExpressionMethods};
use diesel_async::{pooled_connection::AsyncDieselConnectionManager, AsyncPgConnection};
use futures_core::future::BoxFuture;

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

#[async_trait::async_trait]
impl ExecutesRawQuery for PostgresRepo {
    async fn execute_raw_query<'a>(&self, conn: &mut Conn<'a>, query: &str) {
        sql_query(query).execute(conn).await.unwrap();
    }
}

#[async_trait::async_trait]
impl Repo for PostgresRepo {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    async fn get_pool(&self, max_size: u32) -> Pool {
        let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&self.url);

        bb8::Pool::builder()
            .max_size(max_size)
            .build(manager)
            .await
            .unwrap()
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

    async fn stream_contract_addresses<'a, F>(conn: &mut Conn, processor: F)
    where
        F: Fn(Vec<ContractAddress>) -> BoxFuture<'a, ()> + Sync + Send,
    {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        diesel_streamer::stream_serial_table!(chaindexing_contract_addresses, id, conn, processor);
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

    async fn update_last_ingested_block_number<'a>(
        conn: &mut Conn<'a>,
        contract_addresses_list: &Vec<ContractAddress>,
        block_number: i32,
    ) {
        use crate::diesel::schema::chaindexing_contract_addresses::dsl::*;

        let ids = contract_addresses_list.iter().map(|c| c.id);

        diesel::update(chaindexing_contract_addresses)
            .filter(id.eq_any(ids))
            .set(last_ingested_block_number.eq(block_number))
            .execute(conn)
            .await
            .unwrap();
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
