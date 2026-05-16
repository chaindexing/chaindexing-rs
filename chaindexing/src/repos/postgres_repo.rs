mod migrations;
mod raw_queries;

use crate::chain_reorg::UnsavedReorgedBlock;

use crate::chain_blocks::{self, ChainBlock, ConflictingBlock};
use crate::checkpoints::{self, CheckpointKind};
use crate::{contracts::ContractAddress, events::Event, nodes::Node, ChainId};
use diesel::{sql_query, QueryableByName};
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

#[derive(QueryableByName)]
struct AdvisoryLock {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    acquired: bool,
}

impl PostgresRepo {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub(crate) async fn delete_events_from_block_number<'a>(
        conn: &mut Conn<'a>,
        event_chain_id: &ChainId,
        reorged_block_number: i64,
    ) {
        use crate::diesel::schema::chaindexing_events::dsl::*;

        delete(chaindexing_events)
            .filter(chain_id.eq(*event_chain_id as i64))
            .filter(block_number.ge(reorged_block_number))
            .execute(conn)
            .await
            .unwrap();
    }

    pub(crate) async fn sync_blocks<'a>(
        conn: &mut Conn<'a>,
        event_chain_id: &ChainId,
        blocks: &[ChainBlock],
    ) -> Option<i64> {
        let conflict = match chain_blocks::earliest_conflicting_block_query(blocks) {
            Some(query) => sql_query(query)
                .load::<ConflictingBlock>(conn)
                .await
                .unwrap()
                .into_iter()
                .next(),
            None => None,
        };

        if let Some(conflicting_block) = &conflict {
            sql_query(chain_blocks::mark_reorged_from_query(
                *event_chain_id,
                conflicting_block.block_number,
            ))
            .execute(conn)
            .await
            .unwrap();
        }

        if let Some(query) = chain_blocks::upsert_blocks_query(blocks) {
            sql_query(query).execute(conn).await.unwrap();
        }

        conflict.map(|block| block.block_number)
    }

    pub(crate) async fn try_advisory_lock<'a>(conn: &mut Conn<'a>, lock_id: i64) -> bool {
        sql_query(format!(
            "SELECT pg_try_advisory_lock({lock_id}) AS acquired"
        ))
        .load::<AdvisoryLock>(conn)
        .await
        .unwrap()
        .into_iter()
        .next()
        .map(|lock| lock.acquired)
        .unwrap_or(false)
    }
}

#[crate::augmenting_std::async_trait]
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

        if events.is_empty() {
            return;
        }

        diesel::insert_into(chaindexing_events)
            .values(events)
            .on_conflict((
                chain_id,
                contract_address,
                block_hash,
                transaction_hash,
                log_index,
            ))
            .do_nothing()
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

        sql_query(checkpoints::upsert_query(
            contract_address.chain_id as u64,
            &contract_address.address,
            CheckpointKind::Ingestion,
            block_number as u64,
        ))
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
