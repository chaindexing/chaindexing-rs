use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use alloy::network::Ethereum;
use alloy::primitives::B256;
use alloy::providers::{Provider as AlloyProvider, ProviderBuilder};
use alloy::rpc::types::{Block, BlockNumberOrTag, Filter as RpcFilter, Log};
use futures_util::future::try_join_all;
use tokio::time::sleep;

use super::filters::Filter;
use crate::chain_reorg::IndexingFinality;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// Error variant for custom provider implementations.
    ///
    /// This keeps the old ethers-era construction pattern available for
    /// custom `IngesterProvider` implementations while Chaindexing uses Alloy
    /// internally.
    CustomError(String),
    TransportError(String),
}

impl ProviderError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self::CustomError(message.into())
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::CustomError(message) | ProviderError::TransportError(message) => {
                message.fmt(f)
            }
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<alloy::transports::TransportError> for ProviderError {
    fn from(value: alloy::transports::TransportError) -> Self {
        Self::TransportError(value.to_string())
    }
}

const MAX_PROVIDER_ATTEMPTS: u32 = 5;
const MAX_BACKOFF_SECS: u64 = 30;

#[crate::augmenting_std::async_trait]
pub trait Provider: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<u64, ProviderError>;
    async fn get_logs(&self, filter: &RpcFilter) -> Result<Vec<Log>, ProviderError>;

    async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError>;

    async fn get_block_by_tag(
        &self,
        _block_number: BlockNumberOrTag,
    ) -> Result<Option<Block>, ProviderError> {
        Ok(None)
    }

    fn supports_block_hash_log_filters(&self) -> bool {
        true
    }

    async fn get_logs_by_block_hash(
        &self,
        filter: &RpcFilter,
        block_hash: B256,
    ) -> Result<Vec<Log>, ProviderError> {
        self.get_logs(&filter.clone().at_block_hash(block_hash)).await
    }

    async fn get_blocks(
        &self,
        block_numbers: &[u64],
    ) -> Result<HashMap<u64, Block>, ProviderError> {
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
        for block in blocks {
            blocks_by_number.insert(block.header.inner.number, block);
        }

        Ok(blocks_by_number)
    }

    async fn get_blocks_by_number(
        &self,
        logs: &[Log],
    ) -> Result<HashMap<u64, Block>, ProviderError> {
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
    ) -> Result<HashMap<u64, Block>, ProviderError> {
        let block_numbers =
            block_numbers_for_filters(filters, current_block_number, lookback_block_count);

        self.get_blocks(&block_numbers).await
    }
}

#[crate::augmenting_std::async_trait]
impl<P> Provider for P
where
    P: AlloyProvider<Ethereum> + Clone + Send + Sync,
{
    async fn get_block_number(&self) -> Result<u64, ProviderError> {
        AlloyProvider::get_block_number(self).await.map_err(ProviderError::from)
    }

    async fn get_logs(&self, filter: &RpcFilter) -> Result<Vec<Log>, ProviderError> {
        AlloyProvider::get_logs(self, filter).await.map_err(ProviderError::from)
    }

    async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
        AlloyProvider::get_block_by_number(self, BlockNumberOrTag::Number(block_number))
            .await
            .map_err(ProviderError::from)?
            .ok_or_else(|| ProviderError::custom(format!("block {block_number} not found")))
    }

    async fn get_block_by_tag(
        &self,
        block_number: BlockNumberOrTag,
    ) -> Result<Option<Block>, ProviderError> {
        AlloyProvider::get_block_by_number(self, block_number)
            .await
            .map_err(ProviderError::from)
    }
}

pub fn get(json_rpc_url: &str) -> Result<Arc<impl Provider>, ProviderError> {
    let url = json_rpc_url
        .parse()
        .map_err(|error| ProviderError::custom(format!("invalid JSON-RPC URL: {error}")))?;

    Ok(Arc::new(ProviderBuilder::new().connect_http(url)))
}

pub async fn fetch_current_block_number(
    provider: &Arc<impl Provider>,
) -> Result<u64, ProviderError> {
    retry_provider(|| async { provider.get_block_number().await }).await
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
        IndexingFinality::Safe => fetch_tagged_block_number(provider, BlockNumberOrTag::Safe)
            .await
            .map(|block| block.unwrap_or(current_block_number)),
        IndexingFinality::Finalized => {
            fetch_tagged_block_number(provider, BlockNumberOrTag::Finalized)
                .await
                .map(|block| block.unwrap_or(current_block_number))
        }
    }
}

async fn fetch_tagged_block_number(
    provider: &Arc<impl Provider>,
    tag: BlockNumberOrTag,
) -> Result<Option<u64>, ProviderError> {
    retry_provider(|| async {
        provider
            .get_block_by_tag(tag)
            .await
            .map(|block| block.map(|block| block.header.inner.number))
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
    block_hash: B256,
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
    max_in_flight: usize,
    requests_per_second: Option<u32>,
    retry_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
) -> Result<HashMap<u64, Block>, ProviderError> {
    retry_provider_with_policy(
        || async {
            let block_numbers =
                block_numbers_for_filters(filters, current_block_number, lookback_block_count);

            fetch_blocks_with_policy(provider, &block_numbers, max_in_flight, requests_per_second)
                .await
        },
        retry_attempts,
        base_backoff_ms,
        max_backoff_ms,
    )
    .await
}

async fn fetch_blocks_with_policy(
    provider: &Arc<impl Provider>,
    block_numbers: &[u64],
    max_in_flight: usize,
    requests_per_second: Option<u32>,
) -> Result<HashMap<u64, Block>, ProviderError> {
    let max_in_flight = max_in_flight.max(1);
    let mut blocks = vec![];

    for (index, block_number_chunk) in block_numbers.chunks(max_in_flight).enumerate() {
        if index > 0 {
            throttle_requests(block_number_chunk.len(), requests_per_second).await;
        }

        blocks.extend(
            try_join_all(
                block_number_chunk.iter().map(|block_number| provider.get_block(*block_number)),
            )
            .await?,
        );
    }

    Ok(blocks.into_iter().map(|block| (block.header.inner.number, block)).collect())
}

fn block_numbers_for_filters(
    filters: &[Filter],
    current_block_number: u64,
    lookback_block_count: u64,
) -> Vec<u64> {
    filters
        .iter()
        .flat_map(|filter| {
            let from = filter.value.get_from_block().unwrap().saturating_sub(lookback_block_count);
            let to = min(filter.value.get_to_block().unwrap(), current_block_number);

            if from > to {
                vec![]
            } else {
                (from..=to).collect()
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
    use alloy::consensus::Header as ConsensusHeader;
    use alloy::rpc::types::{Filter as RpcFilter, Header as RpcHeader};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn block(number: u64) -> Block {
        Block {
            header: RpcHeader {
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
    fn provider_error_custom_error_variant_keeps_legacy_construction_shape() {
        let error = ProviderError::CustomError("provider failed".to_string());

        assert_eq!(error.to_string(), "provider failed");
        assert_eq!(ProviderError::custom("provider failed"), error);
    }

    #[test]
    fn block_numbers_for_filters_deduplicates_and_clamps_to_current_block() {
        let filters = vec![
            Filter {
                contract_address_id: 1,
                address: "0x1".to_string(),
                value: RpcFilter::new().from_block(10).to_block(12),
            },
            Filter {
                contract_address_id: 2,
                address: "0x2".to_string(),
                value: RpcFilter::new().from_block(11).to_block(20),
            },
        ];

        assert_eq!(
            block_numbers_for_filters(&filters, 13, 0),
            vec![10, 11, 12, 13]
        );
    }

    #[test]
    fn block_numbers_for_filters_includes_lookback_blocks() {
        let filters = vec![Filter {
            contract_address_id: 1,
            address: "0x1".to_string(),
            value: RpcFilter::new().from_block(10).to_block(12),
        }];

        assert_eq!(
            block_numbers_for_filters(&filters, 12, 2),
            vec![8, 9, 10, 11, 12]
        );
    }

    #[derive(Clone)]
    struct TaggedProvider;

    #[crate::augmenting_std::async_trait]
    impl Provider for TaggedProvider {
        async fn get_block_number(&self) -> Result<u64, ProviderError> {
            Ok(100)
        }

        async fn get_logs(&self, _filter: &RpcFilter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
            Ok(block(block_number))
        }

        async fn get_block_by_tag(
            &self,
            block_number: BlockNumberOrTag,
        ) -> Result<Option<Block>, ProviderError> {
            let number = match block_number {
                BlockNumberOrTag::Safe => 90,
                BlockNumberOrTag::Finalized => 80,
                _ => 100,
            };

            Ok(Some(block(number)))
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
        async fn get_block_number(&self) -> Result<u64, ProviderError> {
            Ok(100)
        }

        async fn get_logs(&self, _filter: &RpcFilter) -> Result<Vec<Log>, ProviderError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_observed.fetch_max(current, Ordering::SeqCst);

            tokio::time::sleep(Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            Ok(vec![])
        }

        async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_observed.fetch_max(current, Ordering::SeqCst);

            tokio::time::sleep(Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            Ok(block(block_number))
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
                value: RpcFilter::new(),
            })
            .collect::<Vec<_>>();

        fetch_logs_with_policy(&provider, &filters, 2, None, 5, 1, 10).await.unwrap();

        assert!(max_observed.load(Ordering::SeqCst) <= 2);
    }

    #[tokio::test]
    async fn fetch_blocks_for_filters_with_policy_bounds_in_flight_requests() {
        let max_observed = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(ConcurrencyTrackingProvider {
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_observed: max_observed.clone(),
        });
        let filters = vec![Filter {
            contract_address_id: 1,
            address: "0x1".to_string(),
            value: RpcFilter::new().from_block(1).to_block(5),
        }];

        let blocks =
            fetch_blocks_for_filters_with_policy(&provider, &filters, 5, 0, 2, None, 5, 1, 10)
                .await
                .unwrap();

        assert_eq!(blocks.len(), 5);
        assert!(max_observed.load(Ordering::SeqCst) <= 2);
    }
}
