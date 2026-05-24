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
    max_rpc_in_flight: usize,
    rpc_requests_per_second: Option<u32>,
    rpc_retry_attempts: u32,
    rpc_base_backoff_ms: u64,
    rpc_max_backoff_ms: u64,
) -> Result<FetchedBlockLogs, IngesterError> {
    if provider.supports_block_hash_log_filters() {
        fetch_by_block_hash(
            provider,
            filters,
            chain_id,
            blocks_by_number,
            max_rpc_in_flight,
            rpc_requests_per_second,
            rpc_retry_attempts,
            rpc_base_backoff_ms,
            rpc_max_backoff_ms,
        )
        .await
    } else {
        fetch_by_range(
            provider,
            filters,
            chain_id,
            blocks_by_number,
            max_rpc_in_flight,
            rpc_requests_per_second,
            rpc_retry_attempts,
            rpc_base_backoff_ms,
            rpc_max_backoff_ms,
        )
        .await
    }
}

async fn fetch_by_block_hash(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
    max_rpc_in_flight: usize,
    rpc_requests_per_second: Option<u32>,
    rpc_retry_attempts: u32,
    rpc_base_backoff_ms: u64,
    rpc_max_backoff_ms: u64,
) -> Result<FetchedBlockLogs, IngesterError> {
    let mut logs = vec![];
    let mut scans = vec![];
    let max_rpc_in_flight = max_rpc_in_flight.max(1);
    let mut requests = vec![];

    for filter in filters {
        for block in blocks_for_filter(filter, blocks_by_number) {
            let Some(block_hash) = block.hash else {
                continue;
            };

            requests.push((filter, block_hash));
        }
    }

    for (index, request_chunk) in requests.chunks(max_rpc_in_flight).enumerate() {
        if index > 0 {
            provider::throttle_requests(request_chunk.len(), rpc_requests_per_second).await;
        }

        let chunk_logs =
            futures_util::future::try_join_all(request_chunk.iter().map(|(filter, block_hash)| {
                provider::fetch_logs_by_block_hash_with_policy(
                    provider,
                    filter,
                    *block_hash,
                    rpc_retry_attempts,
                    rpc_base_backoff_ms,
                    rpc_max_backoff_ms,
                )
            }))
            .await?;

        for ((filter, block_hash), mut block_logs) in
            request_chunk.iter().zip(chunk_logs.into_iter())
        {
            block_logs.retain(|log| log.block_hash == Some(*block_hash));

            scans.push(BlockScan::new(
                *chain_id,
                &chain_blocks::h256_to_string(block_hash),
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
    max_rpc_in_flight: usize,
    rpc_requests_per_second: Option<u32>,
    rpc_retry_attempts: u32,
    rpc_base_backoff_ms: u64,
    rpc_max_backoff_ms: u64,
) -> Result<FetchedBlockLogs, IngesterError> {
    let logs = provider::fetch_logs_with_policy(
        provider,
        filters,
        max_rpc_in_flight,
        rpc_requests_per_second,
        rpc_retry_attempts,
        rpc_base_backoff_ms,
        rpc_max_backoff_ms,
    )
    .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::{Filter as EthersFilter, H160, H256};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct ConcurrencyTrackingProvider {
        in_flight: Arc<AtomicUsize>,
        max_observed: Arc<AtomicUsize>,
    }

    #[crate::augmenting_std::async_trait]
    impl Provider for ConcurrencyTrackingProvider {
        async fn get_block_number(&self) -> Result<U64, provider::ProviderError> {
            Ok(U64::from(100))
        }

        async fn get_logs(
            &self,
            _filter: &EthersFilter,
        ) -> Result<Vec<ethers::types::Log>, provider::ProviderError> {
            Ok(vec![])
        }

        async fn get_block(
            &self,
            block_number: U64,
        ) -> Result<Block<TxHash>, provider::ProviderError> {
            Ok(Block {
                number: Some(block_number),
                ..Default::default()
            })
        }

        async fn get_logs_by_block_hash(
            &self,
            _filter: &EthersFilter,
            block_hash: H256,
        ) -> Result<Vec<ethers::types::Log>, provider::ProviderError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_observed.fetch_max(current, Ordering::SeqCst);

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            Ok(vec![ethers::types::Log {
                block_hash: Some(block_hash),
                ..Default::default()
            }])
        }
    }

    #[tokio::test]
    async fn fetch_by_block_hash_bounds_in_flight_requests() {
        let max_observed = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(ConcurrencyTrackingProvider {
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_observed: max_observed.clone(),
        });
        let filters = vec![Filter {
            contract_address_id: 1,
            address: H160::zero().to_string(),
            value: EthersFilter::new().from_block(1).to_block(3),
        }];
        let blocks_by_number = (1..=3)
            .map(|number| {
                (
                    U64::from(number),
                    Block {
                        number: Some(U64::from(number)),
                        hash: Some(H256::from_low_u64_be(number)),
                        ..Default::default()
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        let fetched = fetch_by_block_hash(
            &provider,
            &filters,
            &ChainId::Mainnet,
            &blocks_by_number,
            2,
            None,
            5,
            1,
            10,
        )
        .await
        .unwrap();

        assert_eq!(fetched.logs.len(), 3);
        assert_eq!(fetched.scans.len(), 3);
        assert!(max_observed.load(Ordering::SeqCst) <= 2);
    }
}
