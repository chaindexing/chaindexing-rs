use std::collections::HashMap;
use std::sync::Arc;

use alloy::primitives::B256;
use alloy::rpc::types::{Block, Log};

use crate::chain_blocks::{self, BlockScan};
use crate::ChainId;

use super::filters::Filter;
use super::provider::FetchPolicy;
use super::{provider, IngesterError, Provider};

pub(crate) struct FetchedBlockLogs {
    pub logs: Vec<Log>,
    pub scans: Vec<BlockScan>,
}

pub(crate) async fn fetch(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<u64, Block>,
    policy: FetchPolicy,
) -> Result<FetchedBlockLogs, IngesterError> {
    if provider.supports_block_hash_log_filters() {
        fetch_by_block_hash(provider, filters, chain_id, blocks_by_number, policy).await
    } else {
        fetch_by_range(provider, filters, chain_id, blocks_by_number, policy).await
    }
}

async fn fetch_by_block_hash(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<u64, Block>,
    policy: FetchPolicy,
) -> Result<FetchedBlockLogs, IngesterError> {
    let mut logs = vec![];
    let mut scans = vec![];
    let max_rpc_in_flight = policy.max_in_flight.max(1);
    let mut requests = vec![];

    for filter in filters {
        for block in blocks_for_filter(filter, blocks_by_number) {
            let block_hash = block.header.hash;
            if block_hash == B256::ZERO {
                continue;
            }

            requests.push((filter, block_hash));
        }
    }

    for (index, request_chunk) in requests.chunks(max_rpc_in_flight).enumerate() {
        if index > 0 {
            provider::throttle_requests(request_chunk.len(), policy.requests_per_second).await;
        }

        let chunk_logs =
            futures_util::future::try_join_all(request_chunk.iter().map(|(filter, block_hash)| {
                provider::fetch_logs_by_block_hash_with_policy(
                    provider,
                    filter,
                    *block_hash,
                    policy.retry_attempts,
                    policy.base_backoff_ms,
                    policy.max_backoff_ms,
                )
            }))
            .await?;

        for ((filter, block_hash), mut block_logs) in request_chunk.iter().zip(chunk_logs) {
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
    blocks_by_number: &HashMap<u64, Block>,
    policy: FetchPolicy,
) -> Result<FetchedBlockLogs, IngesterError> {
    let logs = provider::fetch_logs_with_policy(
        provider,
        filters,
        policy.max_in_flight,
        policy.requests_per_second,
        policy.retry_attempts,
        policy.base_backoff_ms,
        policy.max_backoff_ms,
    )
    .await?;
    let mut scans = vec![];

    for filter in filters {
        for block in blocks_for_filter(filter, blocks_by_number) {
            let block_hash = block.header.hash;
            if block_hash == B256::ZERO {
                continue;
            }
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
    blocks_by_number: &'a HashMap<u64, Block>,
) -> Vec<&'a Block> {
    let from = filter.value.get_from_block().unwrap_or(0);
    let to = filter.value.get_to_block().unwrap_or(from);

    let mut blocks: Vec<_> = blocks_by_number
        .iter()
        .filter(|(block_number, _)| **block_number >= from && **block_number <= to)
        .map(|(_, block)| block)
        .collect();

    blocks.sort_by_key(|block| block.header.inner.number);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::consensus::Header as ConsensusHeader;
    use alloy::primitives::Address;
    use alloy::rpc::types::{Filter as RpcFilter, Header as RpcHeader};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[derive(Clone)]
    struct ConcurrencyTrackingProvider {
        in_flight: Arc<AtomicUsize>,
        max_observed: Arc<AtomicUsize>,
    }

    #[crate::augmenting_std::async_trait]
    impl Provider for ConcurrencyTrackingProvider {
        async fn get_block_number(&self) -> Result<u64, provider::ProviderError> {
            Ok(100)
        }

        async fn get_logs(&self, _filter: &RpcFilter) -> Result<Vec<Log>, provider::ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: u64) -> Result<Block, provider::ProviderError> {
            Ok(block(block_number))
        }

        async fn get_logs_by_block_hash(
            &self,
            _filter: &RpcFilter,
            block_hash: B256,
        ) -> Result<Vec<Log>, provider::ProviderError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_observed.fetch_max(current, Ordering::SeqCst);

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            Ok(vec![Log {
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
            address: Address::ZERO.to_string(),
            value: RpcFilter::new().from_block(1).to_block(3),
        }];
        let blocks_by_number =
            (1..=3).map(|number| (number, block(number))).collect::<HashMap<_, _>>();

        let fetched = fetch_by_block_hash(
            &provider,
            &filters,
            &ChainId::Mainnet,
            &blocks_by_number,
            FetchPolicy {
                max_in_flight: 2,
                requests_per_second: None,
                retry_attempts: 5,
                base_backoff_ms: 1,
                max_backoff_ms: 10,
            },
        )
        .await
        .unwrap();

        assert_eq!(fetched.logs.len(), 3);
        assert_eq!(fetched.scans.len(), 3);
        assert!(max_observed.load(Ordering::SeqCst) <= 2);
    }
}
