use std::collections::HashMap;
use std::sync::Arc;

use alloy::primitives::B256;
use alloy::rpc::types::Block;

use crate::indexed_data::{self, IndexedCallTrace, IndexedDataConfig, IndexedTransaction};
use crate::ChainId;

use super::block_logs::FetchPolicy;
use super::filters::Filter;
use super::{provider, IngesterError, Provider};

pub(crate) struct FetchedIndexedData {
    pub transactions: Vec<IndexedTransaction>,
    pub call_traces: Vec<IndexedCallTrace>,
}

impl FetchedIndexedData {
    pub(crate) fn empty() -> Self {
        Self {
            transactions: vec![],
            call_traces: vec![],
        }
    }
}

pub(crate) async fn fetch_for_blocks(
    provider: &Arc<impl Provider>,
    chain_id: &ChainId,
    blocks: &[&Block],
    config: &IndexedDataConfig,
    policy: FetchPolicy,
) -> Result<FetchedIndexedData, IngesterError> {
    let transactions = if config.raw_transactions {
        indexed_data::transactions_from_provider_blocks(chain_id, blocks.iter().copied())
    } else {
        vec![]
    };

    let call_traces = if config.call_traces {
        let traceable_blocks = traceable_blocks(blocks);
        let block_hashes: Vec<_> = traceable_blocks.iter().map(|block| block.header.hash).collect();
        let traces_by_block_hash = provider::fetch_call_traces_by_block_hashes_with_policy(
            provider,
            &block_hashes,
            policy.max_rpc_in_flight,
            policy.requests_per_second,
            policy.retry_attempts,
            policy.base_backoff_ms,
            policy.max_backoff_ms,
        )
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

        traceable_blocks
            .into_iter()
            .flat_map(|block| {
                let traces =
                    traces_by_block_hash.get(&block.header.hash).cloned().unwrap_or_default();

                indexed_data::call_traces_from_provider_traces(chain_id, block, traces)
            })
            .collect()
    } else {
        vec![]
    };

    Ok(FetchedIndexedData {
        transactions,
        call_traces,
    })
}

pub(crate) fn blocks_for_filters<'a>(
    filters: &[Filter],
    blocks_by_number: &'a HashMap<u64, Block>,
) -> Vec<&'a Block> {
    sorted_blocks_matching(blocks_by_number, |block_number| {
        filters.iter().any(|filter| {
            let from = filter.value.get_from_block().unwrap_or(0);
            let to = filter.value.get_to_block().unwrap_or(from);

            block_number >= from && block_number <= to
        })
    })
}

pub(crate) fn blocks_from_fork_point<'a>(
    blocks_by_number: &'a HashMap<u64, Block>,
    fork_point: i64,
) -> Vec<&'a Block> {
    sorted_blocks_matching(blocks_by_number, |block_number| {
        i64::try_from(block_number).map(|number| number >= fork_point).unwrap_or(false)
    })
}

fn sorted_blocks_matching(
    blocks_by_number: &HashMap<u64, Block>,
    mut matches: impl FnMut(u64) -> bool,
) -> Vec<&Block> {
    let mut blocks: Vec<_> = blocks_by_number
        .iter()
        .filter(|(block_number, _)| matches(**block_number))
        .map(|(_, block)| block)
        .collect();

    blocks.sort_by_key(|block| block.header.inner.number);
    blocks
}

fn traceable_blocks<'a>(blocks: &[&'a Block]) -> Vec<&'a Block> {
    blocks.iter().copied().filter(|block| block.header.hash != B256::ZERO).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::consensus::Header as ConsensusHeader;
    use alloy::primitives::Address;
    use alloy::rpc::types::{Filter as RpcFilter, Header as RpcHeader};

    fn h256(value: u64) -> B256 {
        let mut bytes = [0_u8; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        B256::from(bytes)
    }

    fn block(number: u64) -> Block {
        Block {
            header: RpcHeader {
                hash: h256(number),
                inner: ConsensusHeader {
                    number,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn blocks_for_filters_excludes_provider_reorg_lookback_blocks() {
        let filters = vec![Filter {
            contract_address_id: 1,
            address: Address::ZERO.to_string(),
            value: RpcFilter::new().from_block(10).to_block(12),
        }];
        let blocks_by_number =
            (8..=12).map(|number| (number, block(number))).collect::<HashMap<_, _>>();

        let blocks = blocks_for_filters(&filters, &blocks_by_number);
        let block_numbers: Vec<_> = blocks.iter().map(|block| block.header.inner.number).collect();

        assert_eq!(block_numbers, vec![10, 11, 12]);
    }

    #[test]
    fn blocks_from_fork_point_includes_replacement_lookback_blocks() {
        let blocks_by_number =
            (8..=12).map(|number| (number, block(number))).collect::<HashMap<_, _>>();

        let blocks = blocks_from_fork_point(&blocks_by_number, 9);
        let block_numbers: Vec<_> = blocks.iter().map(|block| block.header.inner.number).collect();

        assert_eq!(block_numbers, vec![9, 10, 11, 12]);
    }
}
