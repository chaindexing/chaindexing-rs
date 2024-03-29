mod ingest_events;
mod maybe_handle_chain_reorg;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider, ProviderError};
use ethers::types::{Address, Filter as EthersFilter, Log};
use futures_util::future::try_join_all;
use futures_util::StreamExt;
use std::cmp::min;
use tokio::time::{interval, sleep};
use tokio::{sync::Mutex, task};

use crate::chain_reorg::Execution;
use crate::chains::{Chain, ChainId};
use crate::contracts::Contract;
use crate::contracts::{ContractEventTopic, Contracts};
use crate::pruning::PruningConfig;
use crate::{
    ChaindexingRepo, ChaindexingRepoConn, ChaindexingRepoRawQueryClient, Config, ContractAddress,
    ContractStates, ExecutesWithRawQuery, HasRawQueryClient, MinConfirmationCount, Repo, RepoError,
    Streamable,
};

#[async_trait::async_trait]
pub trait EventsIngesterJsonRpc: Clone + Sync + Send {
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
impl EventsIngesterJsonRpc for Provider<Http> {
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

#[derive(Debug)]
pub enum EventsIngesterError {
    RepoConnectionError,
    GenericError(String),
}

impl From<RepoError> for EventsIngesterError {
    fn from(value: RepoError) -> Self {
        match value {
            RepoError::NotConnected => EventsIngesterError::RepoConnectionError,
            RepoError::Unknown(error) => EventsIngesterError::GenericError(error),
        }
    }
}

#[derive(Clone)]
pub struct EventsIngester;

impl EventsIngester {
    pub fn start<S: Sync + Send + Clone + 'static>(config: &Config<S>) -> task::JoinHandle<()> {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;
            let conn = Arc::new(Mutex::new(conn));

            let raw_query_client = config.repo.get_raw_query_client().await;

            let mut interval = interval(Duration::from_millis(config.ingestion_rate_ms));

            let mut last_pruned_at_per_chain_id = HashMap::new();

            loop {
                for chain @ Chain { json_rpc_url, .. } in config.chains.iter() {
                    let json_rpc = Arc::new(Provider::<Http>::try_from(json_rpc_url).unwrap());

                    Self::ingest(
                        conn.clone(),
                        &raw_query_client,
                        &config.contracts,
                        config.blocks_per_batch,
                        json_rpc,
                        &chain.id,
                        &config.min_confirmation_count,
                        &config.pruning_config,
                        &mut last_pruned_at_per_chain_id,
                    )
                    .await
                    .unwrap();
                }

                interval.tick().await;
            }
        })
    }

    pub async fn ingest<'a, S: Send + Sync + Clone>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        raw_query_client: &ChaindexingRepoRawQueryClient,
        contracts: &Vec<Contract<S>>,
        blocks_per_batch: u64,
        json_rpc: Arc<impl EventsIngesterJsonRpc + 'static>,
        chain_id: &ChainId,
        min_confirmation_count: &MinConfirmationCount,
        pruning_config: &Option<PruningConfig>,
        last_pruned_at_per_chain_id: &mut HashMap<ChainId, u64>,
    ) -> Result<(), EventsIngesterError> {
        let current_block_number = fetch_current_block_number(&json_rpc).await;
        let mut contract_addresses_stream =
            ChaindexingRepo::get_contract_addresses_stream_by_chain(conn.clone(), *chain_id as i64);

        while let Some(contract_addresses) = contract_addresses_stream.next().await {
            let contract_addresses = Self::filter_uningested_contract_addresses(
                &contract_addresses,
                current_block_number,
            );

            let mut conn = conn.lock().await;

            ingest_events::run(
                &mut conn,
                raw_query_client,
                &contract_addresses,
                contracts,
                &json_rpc,
                current_block_number,
                blocks_per_batch,
            )
            .await?;

            maybe_handle_chain_reorg::run(
                &mut conn,
                &contract_addresses,
                contracts,
                &json_rpc,
                chain_id,
                current_block_number,
                blocks_per_batch,
                min_confirmation_count,
            )
            .await?;

            if let Some(pruning_config @ PruningConfig { prune_interval, .. }) = pruning_config {
                let now = Utc::now().timestamp() as u64;
                let last_pruned_at = last_pruned_at_per_chain_id.get(chain_id).unwrap_or(&now);
                let chain_id_u64 = *chain_id as u64;
                if now - *last_pruned_at >= *prune_interval {
                    let min_pruning_block_number =
                        pruning_config.get_min_block_number(current_block_number);
                    ChaindexingRepo::prune_events(
                        raw_query_client,
                        min_pruning_block_number,
                        chain_id_u64,
                    )
                    .await;

                    let state_migrations = Contracts::get_state_migrations(&contracts);
                    let state_table_names = ContractStates::get_all_table_names(&state_migrations);
                    ContractStates::prune_state_versions(
                        &state_table_names,
                        &raw_query_client,
                        min_pruning_block_number,
                        chain_id_u64,
                    )
                    .await;
                }
                last_pruned_at_per_chain_id.insert(*chain_id, Utc::now().timestamp() as u64);
            }
        }

        Ok(())
    }

    fn filter_uningested_contract_addresses(
        contract_addresses: &[ContractAddress],
        current_block_number: u64,
    ) -> Vec<ContractAddress> {
        contract_addresses
            .iter()
            .filter(|ca| current_block_number >= ca.next_block_number_to_ingest_from as u64)
            .cloned()
            .collect()
    }
}

async fn fetch_current_block_number(json_rpc: &Arc<impl EventsIngesterJsonRpc>) -> u64 {
    let mut maybe_current_block_number = None;
    let mut retries_so_far = 0;

    while maybe_current_block_number.is_none() {
        match json_rpc.get_block_number().await {
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
async fn fetch_logs(filters: &[Filter], json_rpc: &Arc<impl EventsIngesterJsonRpc>) -> Vec<Log> {
    let mut maybe_logs = None;
    let mut retries_so_far = 0;

    while maybe_logs.is_none() {
        match try_join_all(filters.iter().map(|f| json_rpc.get_logs(&f.value))).await {
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
async fn fetch_blocks_by_number(
    logs: &Vec<Log>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc>,
) -> HashMap<U64, Block<TxHash>> {
    let mut maybe_blocks_by_number = None;
    let mut retries_so_far = 0;

    while maybe_blocks_by_number.is_none() {
        match json_rpc.get_blocks_by_number(logs).await {
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

struct Filters;

impl Filters {
    fn get<S: Send + Sync + Clone>(
        contract_addresses: &[ContractAddress],
        contracts: &[Contract<S>],
        current_block_number: u64,
        blocks_per_batch: u64,
        execution: &Execution,
    ) -> Vec<Filter> {
        let topics_by_contract_name = Contracts::group_event_topics_by_names(contracts);

        contract_addresses
            .iter()
            .map_while(|contract_address| {
                let topics_by_contract_name =
                    topics_by_contract_name.get(contract_address.contract_name.as_str()).unwrap();

                Filter::maybe_new(
                    contract_address,
                    topics_by_contract_name,
                    current_block_number,
                    blocks_per_batch,
                    execution,
                )
            })
            .collect()
    }

    fn group_by_contract_address_id(filters: &[Filter]) -> HashMap<i32, Vec<Filter>> {
        let empty_filter_group = vec![];

        filters.iter().fold(
            HashMap::new(),
            |mut filters_by_contract_address_id, filter| {
                let mut filter_group = filters_by_contract_address_id
                    .get(&filter.contract_address_id)
                    .unwrap_or(&empty_filter_group)
                    .to_vec();

                filter_group.push(filter.clone());

                filters_by_contract_address_id.insert(filter.contract_address_id, filter_group);

                filters_by_contract_address_id
            },
        )
    }

    fn get_latest(filters: &Vec<Filter>) -> Option<Filter> {
        let mut filters = filters.to_owned();
        filters.sort_by_key(|f| f.value.get_to_block());

        filters.last().cloned()
    }
}

#[derive(Clone, Debug)]
struct Filter {
    contract_address_id: i32,
    address: String,
    value: EthersFilter,
}

impl Filter {
    fn maybe_new(
        contract_address: &ContractAddress,
        topics: &[ContractEventTopic],
        current_block_number: u64,
        blocks_per_batch: u64,
        execution: &Execution,
    ) -> Option<Filter> {
        let ContractAddress {
            id: contract_address_id,
            next_block_number_to_ingest_from,
            start_block_number,
            address,
            ..
        } = contract_address;

        let next_block_number_to_ingest_from = *next_block_number_to_ingest_from as u64;

        match execution {
            Execution::Main => Some((
                next_block_number_to_ingest_from,
                min(
                    next_block_number_to_ingest_from + blocks_per_batch,
                    current_block_number,
                ),
            )),
            Execution::Confirmation(min_confirmation_count) => {
                // TODO: Move logic to higher level
                if min_confirmation_count.is_in_confirmation_window(
                    next_block_number_to_ingest_from,
                    current_block_number,
                ) {
                    Some((
                        min_confirmation_count.deduct_from(
                            next_block_number_to_ingest_from,
                            *start_block_number as u64,
                        ),
                        next_block_number_to_ingest_from + blocks_per_batch,
                    ))
                } else {
                    None
                }
            }
        }
        .map(|(from_block_number, to_block_number)| Filter {
            contract_address_id: *contract_address_id,
            address: address.to_string(),
            value: EthersFilter::new()
                .address(address.parse::<Address>().unwrap())
                .topic0(topics.to_vec())
                .from_block(from_block_number)
                .to_block(to_block_number),
        })
    }
}
