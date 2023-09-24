use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider, ProviderError};
use ethers::types::{Address, Filter as EthersFilter, Log};
use futures_util::future::{join_all, try_join_all};
use futures_util::FutureExt;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::contracts::Contract;
use crate::contracts::{ContractEventTopic, Contracts};
use crate::events::Events;
use crate::{
    ChaindexingRepo, ChaindexingRepoConn, Config, ContractAddress, ContractState, Repo, Streamable,
};

#[async_trait::async_trait]
pub trait EventsIngesterJsonRpc: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<U64, ProviderError>;
    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError>;
}

#[async_trait::async_trait]
impl EventsIngesterJsonRpc for Provider<Http> {
    async fn get_block_number(&self) -> Result<U64, ProviderError> {
        Middleware::get_block_number(&self).await
    }

    async fn get_logs(&self, filter: &EthersFilter) -> Result<Vec<Log>, ProviderError> {
        Middleware::get_logs(&self, filter).await
    }
}

#[derive(Debug)]
pub enum EventsIngesterError {
    HTTPError(String),
    GenericError(String),
}

impl From<ProviderError> for EventsIngesterError {
    fn from(value: ProviderError) -> Self {
        match value {
            ProviderError::HTTPError(error) => EventsIngesterError::HTTPError(error.to_string()),
            other_error => EventsIngesterError::GenericError(other_error.to_string()),
        }
    }
}

#[derive(Clone)]
pub struct EventsIngester;

impl EventsIngester {
    pub fn start<State: ContractState>(config: &Config<State>) {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;
            let conn = Arc::new(Mutex::new(conn));
            let contracts = config.contracts.clone();
            let mut interval = interval(Duration::from_millis(config.ingestion_interval_ms));

            loop {
                interval.tick().await;

                try_join_all(
                    config
                        .chains
                        .clone()
                        .into_iter()
                        .map(|(_chain, json_rpc_url)| {
                            let json_rpc =
                                Arc::new(Provider::<Http>::try_from(json_rpc_url).unwrap());

                            Self::ingest(
                                conn.clone(),
                                &contracts,
                                config.blocks_per_batch,
                                json_rpc,
                            )
                        }),
                )
                .await
                .unwrap();
            }
        });
    }

    // TODO: Can Arc<> or just raw Conn work?
    pub async fn ingest<'a, State: ContractState>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        contracts: &Vec<Contract<State>>,
        blocks_per_batch: u64,
        json_rpc: Arc<impl EventsIngesterJsonRpc + 'static>,
    ) -> Result<(), EventsIngesterError> {
        let current_block_number = (json_rpc.get_block_number().await?).as_u64();

        let mut contract_addresses_stream =
            ChaindexingRepo::get_contract_addresses_stream(conn.clone());

        while let Some(contract_addresses) = contract_addresses_stream.next().await {
            let contract_addresses = Self::filter_uningested_contract_addresses(
                &contract_addresses,
                current_block_number,
            );
            let mut conn = conn.lock().await;
            let filters = Filters::new(
                &contract_addresses,
                &contracts,
                current_block_number,
                blocks_per_batch,
            );
            let logs = Self::fetch_logs(&filters, &json_rpc).await.unwrap();

            let events = Events::new(&logs, &contracts);

            ChaindexingRepo::run_in_transaction(&mut conn, move |conn| {
                async move {
                    ChaindexingRepo::create_events(conn, &events.clone()).await;

                    Self::update_next_block_numbers_to_ingest_from(
                        conn,
                        &contract_addresses,
                        &filters,
                    )
                    .await;

                    Ok(())
                }
                .boxed()
            })
            .await;
        }

        Ok(())
    }

    async fn update_next_block_numbers_to_ingest_from<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: &Vec<ContractAddress>,
        filters: &Vec<Filter>,
    ) {
        let filters_by_contract_address_id = Filters::group_by_contract_address_id(filters);

        let conn = Arc::new(Mutex::new(conn));
        join_all(contract_addresses.iter().map(|contract_address| {
            let filters = filters_by_contract_address_id
                .get(&contract_address.id)
                .unwrap();

            let conn = conn.clone();
            async move {
                if let Some(latest_filter) = Filters::get_latest(filters) {
                    let next_block_number_to_ingest_from =
                        latest_filter.value.get_to_block().unwrap() + 1;

                    let mut conn = conn.lock().await;
                    ChaindexingRepo::update_next_block_number_to_ingest_from(
                        &mut conn,
                        &contract_address,
                        next_block_number_to_ingest_from.as_u64() as i64,
                    )
                    .await
                }
            }
        }))
        .await;
    }

    fn filter_uningested_contract_addresses(
        contract_addresses: &Vec<ContractAddress>,
        current_block_number: u64,
    ) -> Vec<ContractAddress> {
        contract_addresses
            .to_vec()
            .into_iter()
            .filter(|ca| current_block_number > ca.next_block_number_to_ingest_from as u64)
            .collect()
    }

    async fn fetch_logs(
        filters: &Vec<Filter>,
        json_rpc: &Arc<impl EventsIngesterJsonRpc>,
    ) -> Result<Vec<Log>, EventsIngesterError> {
        let logs_per_filter =
            try_join_all(filters.iter().map(|f| json_rpc.get_logs(&f.value))).await?;

        Ok(logs_per_filter.into_iter().flatten().collect())
    }
}

#[derive(Clone, Debug)]
struct Filter {
    contract_address_id: i32,
    value: EthersFilter,
}

impl Filter {
    fn new(
        contract_address: &ContractAddress,
        topics: &Vec<ContractEventTopic>,
        current_block_number: u64,
        blocks_per_batch: u64,
    ) -> Filter {
        let next_block_number_to_ingest_from =
            contract_address.next_block_number_to_ingest_from as u64;

        Filter {
            contract_address_id: contract_address.id,
            value: EthersFilter::new()
                // We could use multiple adddresses here but
                // we'll rather not because it would affect the correctness of
                // next_block_number_to_ingest_from since we stream the contracts upstream.
                .address(contract_address.address.parse::<Address>().unwrap())
                .topic0(topics.to_vec())
                .from_block(next_block_number_to_ingest_from)
                .to_block(std::cmp::min(
                    next_block_number_to_ingest_from + blocks_per_batch,
                    current_block_number,
                )),
        }
    }
}

struct Filters;

impl Filters {
    fn new<State: ContractState>(
        contract_addresses: &Vec<ContractAddress>,
        contracts: &Vec<Contract<State>>,
        current_block_number: u64,
        blocks_per_batch: u64,
    ) -> Vec<Filter> {
        let topics_by_contract_name = Contracts::group_event_topics_by_names(contracts);

        contract_addresses
            .iter()
            .map(|contract_address| {
                let topics_by_contract_name = topics_by_contract_name
                    .get(contract_address.contract_name.as_str())
                    .unwrap();

                Filter::new(
                    contract_address,
                    topics_by_contract_name,
                    current_block_number,
                    blocks_per_batch,
                )
            })
            .collect()
    }

    fn group_by_contract_address_id(filters: &Vec<Filter>) -> HashMap<i32, Vec<Filter>> {
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
        let mut filters = filters.clone();
        filters.sort_by_key(|f| f.value.get_to_block());

        filters.last().cloned()
    }
}
