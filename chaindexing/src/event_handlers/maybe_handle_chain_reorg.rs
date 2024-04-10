use std::sync::Arc;

use tokio::sync::Mutex;

use crate::chain_reorg::{ReorgedBlock, ReorgedBlocks};
use crate::contract_states::ContractStates;
use crate::ChaindexingRepo;
use crate::{
    ChaindexingRepoConn, ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, HasRawQueryClient,
    Repo,
};

pub async fn run<'a>(
    conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
    raw_query_client: &mut ChaindexingRepoRawQueryClient,
    table_names: &Vec<String>,
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
                table_names,
                *chain_id,
                *block_number,
                &raw_query_txn_client,
            )
            .await;
            ChaindexingRepo::update_every_next_block_number_to_handle_from(
                &raw_query_txn_client,
                *chain_id,
                *block_number,
            )
            .await
        }

        let reorged_block_ids = ReorgedBlocks::get_ids(&reorged_blocks);
        ChaindexingRepo::update_reorged_blocks_as_handled(
            &raw_query_txn_client,
            &reorged_block_ids,
        )
        .await;

        ChaindexingRepo::commit_raw_query_txns(raw_query_txn_client).await;
    }
}
