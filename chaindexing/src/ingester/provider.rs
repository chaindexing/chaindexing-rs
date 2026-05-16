use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider as EthersProvider, ProviderError as EthersProviderError};
use ethers::types::{Filter as EthersFilter, Log};
use futures_util::future::try_join_all;
use tokio::time::sleep;

use super::filters::Filter;

pub type ProviderError = EthersProviderError;

#[crate::augmenting_std::async_trait]
pub trait Provider: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<U64, ProviderError>;
    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError>;

    async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError>;

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
        logs: &Vec<Log>,
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
    ) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
        let block_numbers = block_numbers_for_filters(filters, current_block_number);

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
        Ok(Middleware::get_block(&self, block_number).await?.unwrap())
    }
}

pub fn get(json_rpc_url: &str) -> Arc<impl Provider> {
    Arc::new(EthersProvider::<Http>::try_from(json_rpc_url).unwrap())
}

pub async fn fetch_current_block_number(provider: &Arc<impl Provider>) -> u64 {
    let mut maybe_current_block_number = None;
    let mut retries_so_far = 0;

    while maybe_current_block_number.is_none() {
        match provider.get_block_number().await {
            Ok(current_block_number) => {
                maybe_current_block_number = Some(current_block_number.as_u64())
            }
            Err(provider_error) => {
                eprintln!("Provider Error: {provider_error}");

                backoff(retries_so_far).await;
                retries_so_far += 1;
            }
        }
    }

    maybe_current_block_number.unwrap()
}

pub async fn fetch_logs(provider: &Arc<impl Provider>, filters: &[Filter]) -> Vec<Log> {
    let mut maybe_logs = None;
    let mut retries_so_far = 0;

    while maybe_logs.is_none() {
        match try_join_all(filters.iter().map(|f| provider.get_logs(&f.value))).await {
            Ok(logs_per_filter) => {
                let logs = logs_per_filter.into_iter().flatten().collect();

                maybe_logs = Some(logs)
            }
            Err(provider_error) => {
                eprintln!("Provider Error: {provider_error}");

                backoff(retries_so_far).await;
                retries_so_far += 1;
            }
        }
    }

    maybe_logs.unwrap()
}

pub async fn fetch_blocks_for_filters(
    provider: &Arc<impl Provider>,
    filters: &[Filter],
    current_block_number: u64,
) -> HashMap<U64, Block<TxHash>> {
    let mut maybe_blocks_by_number = None;
    let mut retries_so_far = 0;

    while maybe_blocks_by_number.is_none() {
        match provider.get_blocks_for_filters(filters, current_block_number).await {
            Ok(blocks_by_tx_hash) => maybe_blocks_by_number = Some(blocks_by_tx_hash),
            Err(provider_error) => {
                eprintln!("Provider Error: {provider_error}");

                backoff(retries_so_far).await;
                retries_so_far += 1;
            }
        }
    }

    maybe_blocks_by_number.unwrap()
}

fn block_numbers_for_filters(filters: &[Filter], current_block_number: u64) -> Vec<U64> {
    filters
        .iter()
        .flat_map(|filter| {
            let from = filter.value.get_from_block().unwrap().as_u64();
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

async fn backoff(retries_so_far: u32) {
    sleep(Duration::from_secs(2u64.pow(retries_so_far))).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Filter as EthersFilter;

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
            block_numbers_for_filters(&filters, 13),
            vec![U64::from(10), U64::from(11), U64::from(12), U64::from(13)]
        );
    }
}
