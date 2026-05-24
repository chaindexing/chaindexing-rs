use std::collections::HashMap;
use std::sync::Arc;

use ethers::types::{Block, Log, TxHash, U64};

use crate::chain_blocks::{self, BlockScan};
use crate::ChainId;

use super::filters::Filter;
use super::{provider, IngesterError, Provider};

pub(crate) struct FetchedBlockLogs {
    pub logs: Vec<Log>,
    pub scans: Vec<BlockScan>,
}

pub(crate) async fn fetch(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Result<FetchedBlockLogs, IngesterError> {
    if provider.supports_block_hash_log_filters() {
        fetch_by_block_hash(provider, filters, chain_id, blocks_by_number).await
    } else {
        fetch_by_range(provider, filters, chain_id, blocks_by_number).await
    }
}

async fn fetch_by_block_hash(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Result<FetchedBlockLogs, IngesterError> {
    let mut logs = vec![];
    let mut scans = vec![];

    for filter in filters {
        for block in blocks_for_filter(filter, blocks_by_number) {
            let Some(block_hash) = block.hash else {
                continue;
            };
            let block_hash_string = chain_blocks::h256_to_string(&block_hash);
            let mut block_logs =
                provider::fetch_logs_by_block_hash(provider, filter, block_hash).await?;

            block_logs.retain(|log| log.block_hash == Some(block_hash));

            scans.push(BlockScan::new(
                *chain_id,
                &block_hash_string,
                &filter.address,
                &filter.topic_set_key(),
                block_logs.len(),
            ));
            logs.extend(block_logs);
        }
    }

    Ok(FetchedBlockLogs { logs, scans })
}

async fn fetch_by_range(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Result<FetchedBlockLogs, IngesterError> {
    let logs = provider::fetch_logs(provider, filters).await?;
    let mut scans = vec![];

    for filter in filters {
        for block in blocks_for_filter(filter, blocks_by_number) {
            let Some(block_hash) = block.hash else {
                continue;
            };
            let block_hash_string = chain_blocks::h256_to_string(&block_hash);
            let log_count = logs.iter().filter(|log| log.block_hash == Some(block_hash)).count();

            scans.push(BlockScan::new(
                *chain_id,
                &block_hash_string,
                &filter.address,
                &filter.topic_set_key(),
                log_count,
            ));
        }
    }

    Ok(FetchedBlockLogs { logs, scans })
}

fn blocks_for_filter<'a>(
    filter: &Filter,
    blocks_by_number: &'a HashMap<U64, Block<TxHash>>,
) -> Vec<&'a Block<TxHash>> {
    let from = filter.value.get_from_block().unwrap_or_else(|| U64::from(0));
    let to = filter.value.get_to_block().unwrap_or(from);

    let mut blocks: Vec<_> = blocks_by_number
        .iter()
        .filter(|(block_number, _)| **block_number >= from && **block_number <= to)
        .map(|(_, block)| block)
        .collect();

    blocks.sort_by_key(|block| block.number);
    blocks
}
