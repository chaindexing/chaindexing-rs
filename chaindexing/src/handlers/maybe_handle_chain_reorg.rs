use crate::chain_reorg::{ReorgedBlock, ReorgedBlocks};
use crate::{states, ChaindexingRepo, LoadsDataWithRawQuery};
use crate::{ChaindexingRepoClient, ExecutesWithRawQuery, HasRawQueryClient};

pub async fn run(repo_client: &mut ChaindexingRepoClient, table_names: &Vec<String>) {
    let reorged_blocks = ChaindexingRepo::load_unhandled_reorged_blocks(repo_client).await;

    if !reorged_blocks.is_empty() {
        let repo_txn_client = ChaindexingRepo::get_txn_client(repo_client).await;

        let reorged_blocks = ReorgedBlocks::only_earliest_per_chain(&reorged_blocks);

        for ReorgedBlock {
            block_number,
            chain_id,
            ..
        } in &reorged_blocks
        {
            states::backtrack_states(table_names, *chain_id, *block_number, &repo_txn_client).await;
            ChaindexingRepo::update_next_block_numbers_to_handle_from(
                &repo_txn_client,
                *chain_id as u64,
                *block_number as u64,
            )
            .await
        }

        let reorged_block_ids = ReorgedBlocks::get_ids(&reorged_blocks);
        ChaindexingRepo::update_reorged_blocks_as_handled(&repo_txn_client, &reorged_block_ids)
            .await;

        ChaindexingRepo::commit_txns(repo_txn_client).await;
    }
}
