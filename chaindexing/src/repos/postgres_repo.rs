mod migrations;
mod raw_queries;

use crate::chain_reorg::UnsavedReorgedBlock;

use crate::{contracts::ContractAddress, events::Event, nodes::Node};
use diesel_async::RunQueryDsl;

use diesel::{
    delete,
    result::{DatabaseErrorKind, Error as DieselError},
    ExpressionMethods, QueryDsl,
};
use diesel_async::{pooled_connection::AsyncDieselConnectionManager, AsyncPgConnection};
use futures_core::future::BoxFuture;
use uuid::Uuid;

use super::repo::{Repo, RepoError};

pub type Conn<'a> = bb8::PooledConnection<'a, AsyncDieselConnectionManager<AsyncPgConnection>>;
pub type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

pub use diesel_async::{
    scoped_futures::ScopedFutureExt as PostgresRepoTransactionExt,
    AsyncConnection as PostgresRepoAsyncConnection,
};

pub use raw_queries::{PostgresRepoClient, PostgresRepoTxnClient};

impl From<DieselError> for RepoError {
    fn from(value: DieselError) -> Self {
        match value {
            DieselError::DatabaseError(DatabaseErrorKind::ClosedConnection, _info) => {
                RepoError::NotConnected
            }
            any_other_error => RepoError::Unknown(any_other_error.to_string()),
        }
    }
}

/// Repo for Postgres databases
#[derive(Clone, Debug)]
pub struct PostgresRepo {
    url: String,
}

type PgPooledConn<'a> = bb8::PooledConnection<'a, AsyncDieselConnectionManager<AsyncPgConnection>>;

impl PostgresRepo {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Repo for PostgresRepo {
    type Conn<'a> = PgPooledConn<'a>;
    type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

    async fn get_pool(&self, max_size: u32) -> Pool {
        let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&self.url);

        bb8::Pool::builder().max_size(max_size).build(manager).await.unwrap()
    }

    async fn get_conn<'a>(pool: &'a Pool) -> Conn<'a> {
        pool.get().await.unwrap()
    }

    async fn run_in_transaction<'a, F>(conn: &mut Conn<'a>, repo_ops: F) -> Result<(), RepoError>
    where
        F: for<'b> FnOnce(&'b mut Conn<'a>) -> BoxFuture<'b, Result<(), RepoError>>
            + Send
            + Sync
            + 'a,
    {
        conn.transaction::<(), RepoError, _>(|transaction_conn| {
            async move { (repo_ops)(transaction_conn).await }.scope_boxed()
        })
        .await
    }

    async fn create_events<'a>(conn: &mut Conn<'a>, events: &[Event]) {
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
    async fn get_events<'a>(
        conn: &mut Self::Conn<'a>,
        address: String,
        from: u64,
        to: u64,
    ) -> Vec<Event> {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        chaindexing_events
            .filter(contract_address.eq(address.to_lowercase()))
            .filter(block_number.between(from as i64, to as i64))
            .load(conn)
            .await
            .unwrap()
    }
    async fn delete_events_by_ids<'a>(conn: &mut Self::Conn<'a>, ids: &[Uuid]) {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        delete(chaindexing_events).filter(id.eq_any(ids)).execute(conn).await.unwrap();
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

    async fn create_reorged_block<'a>(
        conn: &mut Self::Conn<'a>,
        reorged_block: &UnsavedReorgedBlock,
    ) {
        use crate::diesel::schema::chaindexing_reorged_blocks::dsl::*;

        diesel::insert_into(chaindexing_reorged_blocks)
            .values(reorged_block)
            .execute(conn)
            .await
            .unwrap();
    }

    async fn get_active_nodes<'a>(
        conn: &mut Self::Conn<'a>,
        node_election_rate_ms: u64,
    ) -> Vec<Node> {
        use crate::diesel::schema::chaindexing_nodes::dsl::*;

        chaindexing_nodes
            .filter(last_active_at.gt(Node::get_min_active_at_in_secs(node_election_rate_ms)))
            .load(conn)
            .await
            .unwrap()
    }
    async fn keep_node_active<'a>(conn: &mut Self::Conn<'a>, node: &Node) {
        use crate::diesel::schema::chaindexing_nodes::dsl::*;

        let now = chrono::offset::Utc::now().timestamp();

        diesel::update(chaindexing_nodes)
            .filter(id.eq(node.id))
            .set(last_active_at.eq(now))
            .execute(conn)
            .await
            .unwrap();
    }
}
