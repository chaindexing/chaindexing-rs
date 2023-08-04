use crate::{
    contracts::{ContractAddress, UnsavedContractAddress},
    events::Event,
};
use diesel_async::{scoped_futures::ScopedFutureExt, AsyncConnection, RunQueryDsl};
use diesel_streamer::stream_serial_table;

use diesel::{result::Error, sql_query, upsert::excluded, ExpressionMethods};
use diesel_async::{pooled_connection::AsyncDieselConnectionManager, AsyncPgConnection};
use futures_core::future::BoxFuture;

use super::repo::{Migratable, MigrationList, Repo, SQLikeMigrations};

type Conn<'a> = bb8::PooledConnection<'a, AsyncDieselConnectionManager<AsyncPgConnection>>;
type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

#[derive(Clone)]
pub struct PostgresRepo {
    pool: Pool,
}

#[async_trait::async_trait]
impl<'a> Repo<Conn<'a>> for PostgresRepo {
    async fn new(url: &str) -> Self {
        let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(url);
        let pool = bb8::Pool::builder().build(manager).await.unwrap();

        Self::migrate(|query| Box::pin(Self::exec_raw_query(&pool, query))).await;

        Self { pool }
    }

    async fn create_contract_addresses(&self, contract_addresses: &Vec<UnsavedContractAddress>) {
        use crate::schema::chaindexing_contract_addresses::dsl::{
            address, chaindexing_contract_addresses,
        };

        let mut conn = self.pool.get().await.unwrap();

        diesel::insert_into(chaindexing_contract_addresses)
            .values(contract_addresses)
            .on_conflict(address)
            .do_update()
            .set(address.eq(excluded(address)))
            .execute(&mut conn)
            .await
            .unwrap();
    }

    async fn stream_contract_addresses<'b, F>(&self, processor: F)
    where
        F: Fn(Vec<ContractAddress>) -> BoxFuture<'b, ()> + Sync + std::marker::Send,
    {
        use crate::schema::chaindexing_contract_addresses::dsl::*;

        let mut conn = self.pool.get().await.unwrap();

        diesel_streamer::stream_serial_table!(
            chaindexing_contract_addresses,
            id,
            &mut conn,
            processor
        );
    }
    async fn create_events_and_update_last_ingested_block_number(
        &self,
        events: &Vec<Event>,
        contract_addresses: &Vec<ContractAddress>,
        block_number: i32,
    ) {
        let mut conn = self.pool.get().await.unwrap();

        conn.transaction::<(), Error, _>(|transaction_conn| {
            async move {
                Self::create_events(transaction_conn, events).await;
                Self::update_last_ingested_block_number(
                    transaction_conn,
                    contract_addresses,
                    block_number,
                )
                .await;

                Ok(())
            }
            .scope_boxed()
        })
        .await
        .unwrap();
    }
}

impl PostgresRepo {
    async fn exec_raw_query(pool: &Pool, query: &str) {
        let mut conn = pool.get().await.unwrap();

        sql_query(query).execute(&mut conn).await.unwrap();
    }

    async fn create_events<'a>(conn: &mut Conn<'a>, events: &Vec<Event>) {
        use crate::schema::chaindexing_events::dsl::*;

        diesel::insert_into(chaindexing_events)
            .values(events)
            .execute(conn)
            .await
            .unwrap();
    }

    async fn update_last_ingested_block_number<'a>(
        conn: &mut Conn<'a>,
        contract_addresses_list: &Vec<ContractAddress>,
        block_number: i32,
    ) {
        use crate::schema::chaindexing_contract_addresses::dsl::*;

        let ids = contract_addresses_list.into_iter().map(|c| c.id);

        diesel::update(chaindexing_contract_addresses)
            .filter(id.eq_any(ids))
            .set(last_ingested_block_number.eq(block_number))
            .execute(conn)
            .await
            .unwrap();
    }
}

impl Migratable for PostgresRepo {
    fn create_contract_addresses_migration() -> MigrationList {
        SQLikeMigrations::create_contract_addresses()
    }

    fn create_events_migration() -> MigrationList {
        SQLikeMigrations::create_events()
    }
}
