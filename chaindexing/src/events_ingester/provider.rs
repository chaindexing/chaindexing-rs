use std::collections::HashMap;
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

#[async_trait::async_trait]
pub trait Provider: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<U64, ProviderError>;
    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError>;

    async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError>;
    async fn get_blocks_by_number(
        &self,
        logs: &Vec<Log>,
    ) -> Result<HashMap<U64, Block<TxHash>>, ProviderError> {
        let mut logs = logs.to_owned();
        logs.dedup_by_key(|log| log.block_number);

        const CHUNK_SIZE: usize = 4;
        let chunked_logs: Vec<_> = logs.chunks(CHUNK_SIZE).collect();

        let mut blocks = vec![];
        for chunked_log in chunked_logs {
            blocks.extend(
                try_join_all(
                    chunked_log
                        .iter()
                        .map(|Log { block_number, .. }| self.get_block(block_number.unwrap())),
                )
                .await?,
            );
        }

        let mut blocks_by_number = HashMap::new();
        for block @ Block { number, .. } in blocks {
            blocks_by_number.insert(number.unwrap(), block);
        }

        Ok(blocks_by_number)
    }
}

#[async_trait::async_trait]
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
                eprintln!("Provider Error: {}", provider_error);

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
                eprintln!("Provider Error: {}", provider_error);

                backoff(retries_so_far).await;
                retries_so_far += 1;
            }
        }
    }

    maybe_logs.unwrap()
}

pub async fn fetch_blocks_by_number(
    provider: &Arc<impl Provider>,
    logs: &Vec<Log>,
) -> HashMap<U64, Block<TxHash>> {
    let mut maybe_blocks_by_number = None;
    let mut retries_so_far = 0;

    while maybe_blocks_by_number.is_none() {
        match provider.get_blocks_by_number(logs).await {
            Ok(blocks_by_tx_hash) => maybe_blocks_by_number = Some(blocks_by_tx_hash),
            Err(provider_error) => {
                eprintln!("Provider Error: {}", provider_error);

                backoff(retries_so_far).await;
                retries_so_far += 1;
            }
        }
    }

    maybe_blocks_by_number.unwrap()
}

async fn backoff(retries_so_far: u32) {
    sleep(Duration::from_secs(2u64.pow(retries_so_far))).await;
}
