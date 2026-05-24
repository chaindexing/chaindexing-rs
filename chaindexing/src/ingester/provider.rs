use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider as EthersProvider, ProviderError as EthersProviderError};
use ethers::types::{BlockNumber, Filter as EthersFilter, Log, H256};
use futures_util::future::try_join_all;
use tokio::time::sleep;

use super::filters::Filter;
use crate::chain_reorg::IndexingFinality;

pub type ProviderError = EthersProviderError;
const MAX_PROVIDER_ATTEMPTS: u32 = 5;
const MAX_BACKOFF_SECS: u64 = 30;

#[crate::augmenting_std::async_trait]
pub trait Provider: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<U64, ProviderError>;
    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError>;

    async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError>;

    async fn get_block_by_tag(
        &self,
        _block_number: BlockNumber,
    ) -> Result<Option<Block<TxHash>>, ProviderError> {
        Ok(None)
    }

    fn supports_block_hash_log_filters(&self) -> bool {
        true
    }

    async fn get_logs_by_block_hash(
        &self,
        filter: &EthersFilter,
        block_hash: H256,
    ) -> Result<Vec<Log>, ProviderError> {
        self.get_logs(&filter.clone().at_block_hash(block_hash)).await
    }

    async fn get_blocks(
        &self,
        block_numbers: &[U64],
    ) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
        const CHUNK_SIZE: usize = 8;
        let chunked_block_numbers: Vec<_> = block_numbers.chunks(CHUNK_SIZE).collect();

        let mut blocks = vec![];
        for chunked_block_number in chunked_block_numbers {
            blocks.extend(
                try_join_all(
                    chunked_block_number.iter().map(|block_number| self.get_block(*block_number)),
                )
                .await?,
            );
        }

        let mut blocks_by_number = HashMap::new();
        for block @ Block { number, .. } in blocks {
            if let Some(number) = number {
                blocks_by_number.insert(number, block);
            }
        }

        Ok(blocks_by_number)
    }

    async fn get_blocks_by_number(
        &self,
        logs: &[Log],
    ) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
        let block_numbers: Vec<_> = logs
            .iter()
            .filter_map(|log| log.block_number)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        self.get_blocks(&block_numbers).await
    }

    async fn get_blocks_for_filters(
        &self,
        filters: &[Filter],
        current_block_number: u64,
        lookback_block_count: u64,
    ) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
        let block_numbers =
            block_numbers_for_filters(filters, current_block_number, lookback_block_count);

        self.get_blocks(&block_numbers).await
    }
}

#[crate::augmenting_std::async_trait]
impl Provider for EthersProvider<Http> {
    async fn get_block_number(&self) -> Result<U64, ProviderError> {
        Middleware::get_block_number(&self).await
    }

    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError> {
        Middleware::get_logs(&self, filter).await
    }

    async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
        Middleware::get_block(&self, block_number)
            .await?
            .ok_or_else(|| ProviderError::CustomError(format!("block {block_number} not found")))
    }

    async fn get_block_by_tag(
        &self,
        block_number: BlockNumber,
    ) -> Result<Option<Block<TxHash>>, ProviderError> {
        Middleware::get_block(&self, block_number).await
    }
}

pub fn get(json_rpc_url: &str) -> Result<Arc<impl Provider>, ProviderError> {
    EthersProvider::<Http>::try_from(json_rpc_url)
        .map(Arc::new)
        .map_err(|error| ProviderError::CustomError(format!("invalid JSON-RPC URL: {error}")))
}

pub async fn fetch_current_block_number(
    provider: &Arc<impl Provider>,
) -> Result<u64, ProviderError> {
    retry_provider(|| async { provider.get_block_number().await.map(|block| block.as_u64()) }).await
}

pub async fn fetch_target_block_number(
    provider: &Arc<impl Provider>,
    current_block_number: u64,
    indexing_finality: IndexingFinality,
) -> Result<u64, ProviderError> {
    match indexing_finality {
        IndexingFinality::LatestWithConfirmations(confirmations) => {
            Ok(current_block_number.saturating_sub(confirmations))
        }
        IndexingFinality::Safe => fetch_tagged_block_number(provider, BlockNumber::Safe)
            .await
            .map(|block| block.unwrap_or(current_block_number)),
        IndexingFinality::Finalized => fetch_tagged_block_number(provider, BlockNumber::Finalized)
            .await
            .map(|block| block.unwrap_or(current_block_number)),
    }
}

async fn fetch_tagged_block_number(
    provider: &Arc<impl Provider>,
    tag: BlockNumber,
) -> Result<Option<u64>, ProviderError> {
    retry_provider(|| async {
        provider
            .get_block_by_tag(tag)
            .await
            .map(|block| block.and_then(|block| block.number).map(|number| number.as_u64()))
    })
    .await
}

pub async fn fetch_logs_with_policy(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    max_in_flight: usize,
    requests_per_second: Option<u32>,
    retry_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
) -> Result<Vec<Log>, ProviderError> {
    let max_in_flight = max_in_flight.max(1);

    retry_provider_with_policy(
        || async {
            let mut logs = vec![];

            for (index, filter_chunk) in filters.chunks(max_in_flight).enumerate() {
                if index > 0 {
                    throttle_requests(filter_chunk.len(), requests_per_second).await;
                }

                logs.extend(
                    try_join_all(filter_chunk.iter().map(|f| provider.get_logs(&f.value)))
                        .await?
                        .into_iter()
                        .flatten(),
                );
            }

            Ok(logs)
        },
        retry_attempts,
        base_backoff_ms,
        max_backoff_ms,
    )
    .await
}

pub(crate) async fn throttle_requests(request_count: usize, requests_per_second: Option<u32>) {
    let Some(requests_per_second) = requests_per_second else {
        return;
    };

    if requests_per_second == 0 {
        return;
    }

    let delay = Duration::from_secs_f64(request_count as f64 / requests_per_second as f64);
    sleep(delay).await;
}

pub async fn fetch_logs_by_block_hash_with_policy(
    provider: &Arc<impl Provider>,
    filter: &Filter,
    block_hash: H256,
    retry_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
) -> Result<Vec<Log>, ProviderError> {
    retry_provider_with_policy(
        || async { provider.get_logs_by_block_hash(&filter.value, block_hash).await },
        retry_attempts,
        base_backoff_ms,
        max_backoff_ms,
    )
    .await
}

pub async fn fetch_blocks_for_filters_with_policy(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    current_block_number: u64,
    lookback_block_count: u64,
    retry_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
    retry_provider_with_policy(
        || async {
            provider
                .get_blocks_for_filters(filters, current_block_number, lookback_block_count)
                .await
        },
        retry_attempts,
        base_backoff_ms,
        max_backoff_ms,
    )
    .await
}

fn block_numbers_for_filters(
    filters: &[Filter],
    current_block_number: u64,
    lookback_block_count: u64,
) -> Vec<U64> {
    filters
        .iter()
        .flat_map(|filter| {
            let from = filter
                .value
                .get_from_block()
                .unwrap()
                .as_u64()
                .saturating_sub(lookback_block_count);
            let to = min(
                filter.value.get_to_block().unwrap().as_u64(),
                current_block_number,
            );

            if from > to {
                vec![]
            } else {
                (from..=to).map(U64::from).collect()
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn retry_provider<T, F, Fut>(mut operation: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    retry_provider_with_policy(
        &mut operation,
        MAX_PROVIDER_ATTEMPTS,
        1_000,
        MAX_BACKOFF_SECS * 1_000,
    )
    .await
}

async fn retry_provider_with_policy<T, F, Fut>(
    mut operation: F,
    max_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    let mut attempts = 0;
    let max_attempts = max_attempts.max(1);
    let base_backoff_ms = base_backoff_ms.max(1);
    let max_backoff_ms = max_backoff_ms.max(base_backoff_ms);

    loop {
        attempts += 1;

        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if attempts >= max_attempts => return Err(error),
            Err(_) => backoff(attempts - 1, base_backoff_ms, max_backoff_ms).await,
        }
    }
}

async fn backoff(retries_so_far: u32, base_backoff_ms: u64, max_backoff_ms: u64) {
    let delay_ms = base_backoff_ms
        .saturating_mul(2u64.saturating_pow(retries_so_far))
        .min(max_backoff_ms);

    sleep(Duration::from_millis(delay_ms)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Filter as EthersFilter;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn block_numbers_for_filters_deduplicates_and_clamps_to_current_block() {
        let filters = vec![
            Filter {
                contract_address_id: 1,
                address: "0x1".to_string(),
                value: EthersFilter::new().from_block(10).to_block(12),
            },
            Filter {
                contract_address_id: 2,
                address: "0x2".to_string(),
                value: EthersFilter::new().from_block(11).to_block(20),
            },
        ];

        assert_eq!(
            block_numbers_for_filters(&filters, 13, 0),
            vec![U64::from(10), U64::from(11), U64::from(12), U64::from(13)]
        );
    }

    #[test]
    fn block_numbers_for_filters_includes_lookback_blocks() {
        let filters = vec![Filter {
            contract_address_id: 1,
            address: "0x1".to_string(),
            value: EthersFilter::new().from_block(10).to_block(12),
        }];

        assert_eq!(
            block_numbers_for_filters(&filters, 12, 2),
            vec![
                U64::from(8),
                U64::from(9),
                U64::from(10),
                U64::from(11),
                U64::from(12)
            ]
        );
    }

    #[derive(Clone)]
    struct TaggedProvider;

    #[crate::augmenting_std::async_trait]
    impl Provider for TaggedProvider {
        async fn get_block_number(&self) -> Result<U64, ProviderError> {
            Ok(U64::from(100))
        }

        async fn get_logs(&self, _filter: &EthersFilter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
            Ok(Block {
                number: Some(block_number),
                ..Default::default()
            })
        }

        async fn get_block_by_tag(
            &self,
            block_number: BlockNumber,
        ) -> Result<Option<Block<TxHash>>, ProviderError> {
            let number = match block_number {
                BlockNumber::Safe => 90,
                BlockNumber::Finalized => 80,
                _ => 100,
            };

            Ok(Some(Block {
                number: Some(U64::from(number)),
                ..Default::default()
            }))
        }
    }

    #[tokio::test]
    async fn fetch_target_block_number_uses_tagged_finality_when_available() {
        let provider = Arc::new(TaggedProvider);

        assert_eq!(
            fetch_target_block_number(&provider, 100, IndexingFinality::Safe).await.unwrap(),
            90
        );
        assert_eq!(
            fetch_target_block_number(&provider, 100, IndexingFinality::Finalized)
                .await
                .unwrap(),
            80
        );
        assert_eq!(
            fetch_target_block_number(
                &provider,
                100,
                IndexingFinality::LatestWithConfirmations(12),
            )
            .await
            .unwrap(),
            88
        );
    }

    #[derive(Clone)]
    struct ConcurrencyTrackingProvider {
        in_flight: Arc<AtomicUsize>,
        max_observed: Arc<AtomicUsize>,
    }

    #[crate::augmenting_std::async_trait]
    impl Provider for ConcurrencyTrackingProvider {
        async fn get_block_number(&self) -> Result<U64, ProviderError> {
            Ok(U64::from(100))
        }

        async fn get_logs(&self, _filter: &EthersFilter) -> Result<Vec<Log>, ProviderError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_observed.fetch_max(current, Ordering::SeqCst);

            tokio::time::sleep(Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            Ok(vec![])
        }

        async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
            Ok(Block {
                number: Some(block_number),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn fetch_logs_with_policy_bounds_in_flight_requests() {
        let max_observed = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(ConcurrencyTrackingProvider {
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_observed: max_observed.clone(),
        });
        let filters = (0..5)
            .map(|_| Filter {
                contract_address_id: 1,
                address: "0x1".to_string(),
                value: EthersFilter::new(),
            })
            .collect::<Vec<_>>();

        fetch_logs_with_policy(&provider, &filters, 2, None, 5, 1, 10).await.unwrap();

        assert!(max_observed.load(Ordering::SeqCst) <= 2);
    }
}
