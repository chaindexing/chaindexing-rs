use std::sync::Arc;
use std::time::Duration;

use ethers::prelude::Middleware;
use ethers::prelude::*;
use ethers::providers::{Http, Provider, ProviderError};
use ethers::types::{Address, Filter, Log};
use futures_util::future::try_join_all;
use futures_util::FutureExt;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::contracts::Contract;
use crate::contracts::{ContractEventTopic, Contracts};
use crate::events::Events;
use crate::{ChaindexingRepo, ChaindexingRepoConn, Config, ContractAddress, Repo};

#[async_trait::async_trait]
pub trait EventsIngesterJsonRpc: Clone + Sync + Send {
    async fn get_block_number(&self) -> Result<U64, ProviderError>;
    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError>;
}

#[async_trait::async_trait]
impl EventsIngesterJsonRpc for Provider<Http> {
    async fn get_block_number(&self) -> Result<U64, ProviderError> {
        Middleware::get_block_number(&self).await
    }

    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError> {
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
    pub fn start(config: &Config) {
        let config = config.clone();

        tokio::spawn(async move {
            let pool = config.repo.get_pool(2).await;
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
    pub async fn ingest<'a>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        contracts: &Vec<Contract>,
        blocks_per_batch: u64,
        json_rpc: Arc<impl EventsIngesterJsonRpc + 'static>,
    ) -> Result<(), EventsIngesterError> {
        let current_block_number = json_rpc.get_block_number().await?;

        let mut contract_addresses_streamer =
            ChaindexingRepo::get_contract_addresses_streamer(conn.clone()).await;

        while let Some(contract_addresses) = contract_addresses_streamer.next().await {
            let mut conn = conn.lock().await;
            let filters = build_filters(
                &contract_addresses,
                &contracts,
                current_block_number.as_u64(),
                blocks_per_batch,
            );
            let logs = fetch_logs(&filters, &json_rpc).await.unwrap();
            let events = Events::new(&logs, &contracts);

            ChaindexingRepo::run_in_transaction(&mut conn, move |conn| {
                async move {
                    ChaindexingRepo::create_events(conn, &events.clone()).await;

                    ChaindexingRepo::update_last_ingested_block_number(
                        conn,
                        &contract_addresses.clone(),
                        current_block_number.as_u32() as i32,
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
}

async fn fetch_logs(
    filters: &Vec<Filter>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc>,
) -> Result<Vec<Log>, EventsIngesterError> {
    let logs_per_filter = try_join_all(filters.iter().map(|f| json_rpc.get_logs(&f))).await?;

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
        // we'll rather not because it would affect the correctness of
        // last_ingested_block_number since we stream the contracts upstream.
        .address(contract_address.address.parse::<Address>().unwrap())
        .topic0(topics.to_vec())
        .from_block(last_ingested_block_number)
        .to_block(std::cmp::min(
            last_ingested_block_number + blocks_per_batch,
            current_block_number,
        ))
}
