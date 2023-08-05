use std::time::Duration;

use ethers::prelude::*;
use ethers::providers::{Http, Provider, ProviderError};
use ethers::types::{Address, Filter, Log};
use futures_util::future::try_join_all;
use futures_util::FutureExt;
use tokio::time::interval;

use crate::chains::JsonRpcUrl;
use crate::contracts::Contract;
use crate::contracts::{ContractEventTopic, Contracts};
use crate::events::Events;
use crate::{ChaindexingRepo, Config, ContractAddress, Repo};

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
    pub fn start(config: &Config) {
        let config = config.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(config.ingestion_interval_ms));

            loop {
                interval.tick().await;

                try_join_all(config.chains.clone().into_iter().map(
                    |(_chain, JsonRpcUrl(json_rpc_url))| {
                        EventsIngester::ingest(&config, json_rpc_url)
                    },
                ))
                .await
                .unwrap();
            }
        });
    }

    async fn ingest(config: &Config, json_rpc_url: String) -> Result<(), EventsIngesterError> {
        let repo = &config.repo;
        let pool = &repo.get_pool().await;
        let mut conn = ChaindexingRepo::get_conn(pool).await;

        let current_block_number = json_rpc(&json_rpc_url).get_block_number().await?;
        let contracts = &config.contracts;
        let json_rpc_url_ = json_rpc_url.as_str();
        let blocks_per_batch = config.blocks_per_batch;

        ChaindexingRepo::stream_contract_addresses(
            &mut conn,
            |contract_addresses: Vec<ContractAddress>| {
                async move {
                    let filters = build_filters(
                        &contract_addresses,
                        contracts,
                        current_block_number.as_u64(),
                        blocks_per_batch,
                    );

                    let logs = fetch_logs(&filters, &json_rpc(json_rpc_url_))
                        .await
                        .unwrap();

                    let events = Events::new(&logs, contracts);

                    let mut conn_for_transaction = ChaindexingRepo::get_conn(pool).await;

                    ChaindexingRepo::run_in_transaction(
                        &mut conn_for_transaction,
                        |transaction_conn| {
                            async move {
                                ChaindexingRepo::create_events(transaction_conn, &events.clone())
                                    .await;

                                ChaindexingRepo::update_last_ingested_block_number(
                                    transaction_conn,
                                    &contract_addresses.clone(),
                                    current_block_number.as_u32() as i32,
                                )
                                .await;

                                Ok(())
                            }
                            .boxed()
                        },
                    )
                    .await;
                }
                .boxed()
            },
        )
        .await;

        Ok(())
    }
}

fn json_rpc(json_rpc_url: &str) -> Provider<Http> {
    Provider::<Http>::try_from(json_rpc_url).unwrap()
}

async fn fetch_logs(
    filters: &Vec<Filter>,
    json_rpc: &Provider<Http>,
) -> Result<Vec<Log>, EventsIngesterError> {
    let logs_per_filter = try_join_all(filters.into_iter().map(|f| json_rpc.get_logs(&f))).await?;

    Ok(logs_per_filter.into_iter().flatten().collect())
}

pub fn build_filters(
    contract_addresses: &Vec<ContractAddress>,
    contracts: &Vec<Contract>,
    current_block_number: u64,
    blocks_per_batch: u64,
) -> Vec<Filter> {
    let topics_by_contract_name = Contracts::group_event_topics_by_names(contracts);

    contract_addresses
        .iter()
        .map(|ca| {
            build_filter(
                ca,
                topics_by_contract_name
                    .get(ca.contract_name.as_str())
                    .unwrap(),
                current_block_number,
                blocks_per_batch,
            )
        })
        .collect()
}

fn build_filter(
    contract_address: &ContractAddress,
    topics: &Vec<ContractEventTopic>,
    current_block_number: u64,
    blocks_per_batch: u64,
) -> Filter {
    let last_ingested_block_number = contract_address.last_ingested_block_number as u64;

    Filter::new()
        // We could use multiple adddresses here but
        // we'll rather not because it could lead to problems with the last_ingested_block_number
        // because we stream the contracts upstream.
        .address(contract_address.address.parse::<Address>().unwrap())
        .topic0(topics.to_vec())
        .from_block(last_ingested_block_number)
        .to_block(std::cmp::min(
            last_ingested_block_number + blocks_per_batch,
            current_block_number,
        ))
}
