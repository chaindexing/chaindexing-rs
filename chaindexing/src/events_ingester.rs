use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider, ProviderError};
use ethers::types::{Address, Filter as EthersFilter, Log};
use futures_util::future::try_join_all;
use futures_util::FutureExt;
use futures_util::StreamExt;
use std::cmp::{max, min};
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::chain_reorg::{Execution, UnsavedReorgedBlock};
use crate::contracts::Contract;
use crate::contracts::{ContractEventTopic, Contracts};
use crate::events::{Event, Events};
use crate::{
    ChaindexingRepo, ChaindexingRepoConn, Config, ContractAddress, MinConfirmationCount, Repo,
    RepoError, Streamable,
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
    RepoConnectionError,
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
    pub fn start(config: &Config) {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;
            let conn = Arc::new(Mutex::new(conn));
            let contracts = config.contracts.clone();
            let mut interval = interval(Duration::from_millis(config.ingestion_interval_ms));
            let mut run_confirmation_execution = false;

            loop {
                interval.tick().await;

                for (chain, json_rpc_url) in config.chains.clone() {
                    let json_rpc = Arc::new(Provider::<Http>::try_from(json_rpc_url).unwrap());

                    Self::ingest(
                        conn.clone(),
                        &contracts,
                        config.blocks_per_batch,
                        json_rpc,
                        &chain,
                        &config.min_confirmation_count,
                        run_confirmation_execution,
                    )
                    .await
                    .unwrap();
                }

                if !run_confirmation_execution {
                    run_confirmation_execution = true;
                }
            }
        });
    }

    pub async fn ingest<'a>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        contracts: &Vec<Contract>,
        blocks_per_batch: u64,
        json_rpc: Arc<impl EventsIngesterJsonRpc + 'static>,
        chain: &Chain,
        min_confirmation_count: &MinConfirmationCount,
        run_confirmation_execution: bool,
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

            MainExecution::ingest(
                &mut conn,
                contract_addresses.clone(),
                contracts,
                &json_rpc,
                chain,
                current_block_number,
                blocks_per_batch,
            )
            .await?;

            if run_confirmation_execution {
                ConfirmationExecution::ingest(
                    &mut conn,
                    contract_addresses.clone(),
                    contracts,
                    &json_rpc,
                    chain,
                    current_block_number,
                    blocks_per_batch,
                    min_confirmation_count,
                )
                .await?;
            }
        }

        Ok(())
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
}

struct MainExecution;

impl MainExecution {
    async fn ingest<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: Vec<ContractAddress>,
        contracts: &Vec<Contract>,
        json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
        chain: &Chain,
        current_block_number: u64,
        blocks_per_batch: u64,
    ) -> Result<(), EventsIngesterError> {
        let filters = Filters::new(
            &contract_addresses,
            &contracts,
            current_block_number,
            blocks_per_batch,
            &Execution::Main,
        );

        if !filters.is_empty() {
            let logs = fetch_logs(&filters, json_rpc).await.unwrap();
            let events = Events::new(&logs, &contracts, chain);

            ChaindexingRepo::run_in_transaction(conn, move |conn| {
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
            .await?;
        }

        Ok(())
    }

    async fn update_next_block_numbers_to_ingest_from<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: &Vec<ContractAddress>,
        filters: &Vec<Filter>,
    ) {
        let filters_by_contract_address_id = Filters::group_by_contract_address_id(filters);

        for contract_address in contract_addresses {
            let filters = filters_by_contract_address_id.get(&contract_address.id).unwrap();

            if let Some(latest_filter) = Filters::get_latest(filters) {
                let next_block_number_to_ingest_from =
                    latest_filter.value.get_to_block().unwrap() + 1;

                ChaindexingRepo::update_next_block_number_to_ingest_from(
                    conn,
                    &contract_address,
                    next_block_number_to_ingest_from.as_u64() as i64,
                )
                .await
            }
        }
    }
}

struct ConfirmationExecution;

impl ConfirmationExecution {
    async fn ingest<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: Vec<ContractAddress>,
        contracts: &Vec<Contract>,
        json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
        chain: &Chain,
        current_block_number: u64,
        blocks_per_batch: u64,
        min_confirmation_count: &MinConfirmationCount,
    ) -> Result<(), EventsIngesterError> {
        let filters = Filters::new(
            &contract_addresses,
            &contracts,
            current_block_number,
            blocks_per_batch,
            &Execution::Confirmation(min_confirmation_count),
        );

        if !filters.is_empty() {
            let logs = fetch_logs(&filters, json_rpc).await.unwrap();

            if let Some((min_block_number, max_block_number)) =
                Self::get_min_and_max_block_number(&logs)
            {
                let already_ingested_events =
                    ChaindexingRepo::get_events(conn, min_block_number, max_block_number).await;

                let json_rpc_events = Events::new(&logs, &contracts, chain);

                Self::maybe_handle_chain_reorg(
                    conn,
                    chain,
                    &already_ingested_events,
                    &json_rpc_events,
                )
                .await?;
            }
        }

        Ok(())
    }

    fn get_min_and_max_block_number(logs: &Vec<Log>) -> Option<(u64, u64)> {
        logs.clone().iter().fold(None, |block_range, Log { block_number, .. }| {
            let block_number = block_number.unwrap().as_u64();

            block_range
                .and_then(|(min_block_number, max_block_number)| {
                    let min_block_number = min(min_block_number, block_number);
                    let max_block_number = max(max_block_number, block_number);

                    Some((min_block_number, max_block_number))
                })
                .or_else(|| Some((block_number, block_number)))
        })
    }

    async fn maybe_handle_chain_reorg<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        chain: &Chain,
        already_ingested_events: &Vec<Event>,
        json_rpc_events: &Vec<Event>,
    ) -> Result<(), EventsIngesterError> {
        if let Some((added_events, removed_events)) =
            Self::get_json_rpc_added_and_removed_events(&already_ingested_events, &json_rpc_events)
        {
            let earliest_block_number =
                Self::get_earliest_block_number((&added_events, &removed_events));

            let new_reorged_block = UnsavedReorgedBlock::new(earliest_block_number, chain);

            ChaindexingRepo::run_in_transaction(conn, move |conn| {
                async move {
                    ChaindexingRepo::create_reorged_block(conn, &new_reorged_block).await;

                    let event_ids = removed_events.iter().map(|e| e.id).collect();
                    ChaindexingRepo::delete_events_by_ids(conn, &event_ids).await;

                    ChaindexingRepo::create_events(conn, &added_events).await;

                    Ok(())
                }
                .boxed()
            })
            .await?;
        }

        Ok(())
    }

    fn get_json_rpc_added_and_removed_events(
        already_ingested_events: &Vec<Event>,
        json_rpc_events: &Vec<Event>,
    ) -> Option<(Vec<Event>, Vec<Event>)> {
        let already_ingested_events_set: HashSet<_> =
            already_ingested_events.clone().into_iter().collect();
        let json_rpc_events_set: HashSet<_> = json_rpc_events.clone().into_iter().collect();

        let added_events: Vec<_> = json_rpc_events
            .clone()
            .into_iter()
            .filter(|e| !already_ingested_events_set.contains(e))
            .collect();

        let removed_events: Vec<_> = already_ingested_events
            .clone()
            .into_iter()
            .filter(|e| !json_rpc_events_set.contains(e))
            .collect();

        if added_events.is_empty() && removed_events.is_empty() {
            None
        } else {
            Some((added_events, removed_events))
        }
    }

    fn get_earliest_block_number((add_events, removed_events): (&Vec<Event>, &Vec<Event>)) -> i64 {
        let earliest_block_number =
            add_events.iter().min_by_key(|e| e.block_number).unwrap().block_number;
        let earliest_block_number = min(
            earliest_block_number,
            removed_events.iter().min_by_key(|e| e.block_number).unwrap().block_number,
        );

        earliest_block_number
    }
}

async fn fetch_logs(
    filters: &Vec<Filter>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc>,
) -> Result<Vec<Log>, EventsIngesterError> {
    let logs_per_filter = try_join_all(filters.iter().map(|f| json_rpc.get_logs(&f.value))).await?;

    Ok(logs_per_filter.into_iter().flatten().collect())
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
        execution: &Execution,
    ) -> Filter {
        let ContractAddress {
            id: contract_address_id,
            next_block_number_to_ingest_from,
            start_block_number,
            address,
            ..
        } = contract_address;

        let next_block_number_to_ingest_from = match execution {
            Execution::Main => *next_block_number_to_ingest_from as u64,
            Execution::Confirmation(min_confirmation_count) => min_confirmation_count
                .ideduct_from(*next_block_number_to_ingest_from, *start_block_number),
        };

        let current_block_number = match execution {
            Execution::Main => current_block_number,
            Execution::Confirmation(min_confirmation_count) => {
                min_confirmation_count.deduct_from(current_block_number, *start_block_number as u64)
            }
        };

        Filter {
            contract_address_id: *contract_address_id,
            value: EthersFilter::new()
                .address(address.parse::<Address>().unwrap())
                .topic0(topics.to_vec())
                .from_block(next_block_number_to_ingest_from)
                .to_block(min(
                    next_block_number_to_ingest_from + blocks_per_batch,
                    current_block_number,
                )),
        }
    }
}

struct Filters;

impl Filters {
    fn new(
        contract_addresses: &Vec<ContractAddress>,
        contracts: &Vec<Contract>,
        current_block_number: u64,
        blocks_per_batch: u64,
        execution: &Execution,
    ) -> Vec<Filter> {
        let topics_by_contract_name = Contracts::group_event_topics_by_names(contracts);

        contract_addresses
            .iter()
            .map(|contract_address| {
                let topics_by_contract_name =
                    topics_by_contract_name.get(contract_address.contract_name.as_str()).unwrap();

                Filter::new(
                    contract_address,
                    topics_by_contract_name,
                    current_block_number,
                    blocks_per_batch,
                    execution,
                )
            })
            .filter(|f| !f.value.get_from_block().eq(&f.value.get_to_block()))
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
