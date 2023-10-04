use std::sync::Arc;

use tokio::sync::Mutex;

use crate::ChaindexingRepo;
use crate::{
    ChaindexingRepoConn, ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, HasRawQueryClient,
    Repo,
};
use crate::{ContractStateMigrations, ContractStates};
use crate::{ReorgedBlock, ReorgedBlocks};

pub struct MaybeBacktrackHandledEvents;

impl MaybeBacktrackHandledEvents {
    pub async fn run<'a>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        raw_query_client: &mut ChaindexingRepoRawQueryClient,
        state_migrations: &Vec<Arc<dyn ContractStateMigrations>>,
    ) {
        let mut conn = conn.lock().await;
        let reorged_blocks = ChaindexingRepo::get_unhandled_reorged_blocks(&mut conn).await;

        if !reorged_blocks.is_empty() {
            let raw_query_txn_client =
                ChaindexingRepo::get_raw_query_txn_client(raw_query_client).await;

            let reorged_blocks = ReorgedBlocks::only_earliest_per_chain(&reorged_blocks);

            for ReorgedBlock {
                block_number,
                chain_id,
                ..
            } in &reorged_blocks
            {
                ContractStates::backtrack_states(
                    state_migrations,
                    *chain_id,
                    *block_number,
                    &raw_query_txn_client,
                )
                .await;
                ChaindexingRepo::update_every_next_block_number_to_handle_from_in_txn(
                    &raw_query_txn_client,
                    *chain_id,
                    *block_number,
                )
                .await
            }

            let reorged_block_ids = ReorgedBlocks::get_ids(&reorged_blocks);
            ChaindexingRepo::update_reorged_blocks_as_handled_in_txn(
                &raw_query_txn_client,
                &reorged_block_ids,
            )
            .await;

            ChaindexingRepo::commit_raw_query_txns(raw_query_txn_client).await;
        }
    }
}
