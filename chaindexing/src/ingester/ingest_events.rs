use std::collections::HashMap;
use std::sync::Arc;

use futures_util::FutureExt;

use super::filters::{self, Filter};
use super::provider::{self, Provider};
use super::IngesterError;

use crate::chain_reorg::Execution;
use crate::Config;
use crate::{events, ChainId};
use crate::{
    ChaindexingRepo, ChaindexingRepoClient, ChaindexingRepoConn, ContractAddress,
    LoadsDataWithRawQuery, Repo,
};

pub async fn run<'a, S: Send + Sync + Clone>(
    conn: &mut ChaindexingRepoConn<'a>,
    repo_client: &ChaindexingRepoClient,
    contract_addresses: Vec<ContractAddress>,
    provider: &Arc<impl Provider>,
    chain_id: &ChainId,
    current_block_number: u64,
    Config {
        contracts,
        blocks_per_batch,
        ..
    }: &Config<S>,
) -> Result<(), IngesterError> {
    let filters = filters::get(
        &contract_addresses,
        contracts,
        current_block_number,
        *blocks_per_batch,
        &Execution::Main,
    );

    let filters = remove_already_ingested_filters(&filters, &contract_addresses, repo_client).await;

    if !filters.is_empty() {
        let logs = provider::fetch_logs(provider, &filters).await;
        let blocks_by_tx_hash = provider::fetch_blocks_by_number(provider, &logs).await;
        let events = events::get(
            &logs,
            contracts,
            &contract_addresses,
            chain_id,
            &blocks_by_tx_hash,
        );
        let contract_addresses = contract_addresses.clone();

        ChaindexingRepo::run_in_transaction(conn, move |conn| {
            async move {
                ChaindexingRepo::create_events(conn, &events.clone()).await;

                update_next_block_numbers_to_ingest_from(conn, &contract_addresses, &filters).await;

                Ok(())
            }
            .boxed()
        })
        .await?;
    }

    Ok(())
}

async fn remove_already_ingested_filters(
    filters: &Vec<Filter>,
    contract_addresses: &[ContractAddress],
    repo_client: &ChaindexingRepoClient,
) -> Vec<Filter> {
    let current_block_filters: Vec<_> = filters
        .iter()
        .filter(|f| f.value.get_from_block() == f.value.get_to_block())
        .collect();

    if current_block_filters.is_empty() {
        filters.to_owned()
    } else {
        let addresses: Vec<_> = contract_addresses.iter().map(|c| c.address.clone()).collect();

        let latest_ingested_events =
            ChaindexingRepo::load_latest_events(repo_client, &addresses).await;
        let latest_ingested_events =
            latest_ingested_events
                .iter()
                .fold(HashMap::new(), |mut events_by_address, event| {
                    events_by_address.insert(&event.contract_address, event);

                    events_by_address
                });

        let already_ingested_filters = current_block_filters
            .iter()
            .filter(|filter| match latest_ingested_events.get(&filter.address) {
                Some(latest_event) => {
                    latest_event.block_number as u64
                        == filter.value.get_to_block().unwrap().as_u64()
                }
                None => false,
            })
            .fold(HashMap::new(), |mut stale_current_block_filters, filter| {
                stale_current_block_filters.insert(filter.contract_address_id, filter);

                stale_current_block_filters
            });

        filters
            .iter()
            .filter(|f| !already_ingested_filters.contains_key(&f.contract_address_id))
            .cloned()
            .collect::<Vec<_>>()
    }
}

async fn update_next_block_numbers_to_ingest_from<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    contract_addresses: &[ContractAddress],
    filters: &[Filter],
) {
    let filters_by_contract_address_id = filters::group_by_contract_address_id(filters);

    for (contract_address, filters) in contract_addresses
        .iter()
        .filter_map(|ca| filters_by_contract_address_id.get(&ca.id).map(|f| (ca, f)))
    {
        if let Some(latest_filter) = filters::get_latest(filters) {
            let next_block_number_to_ingest_from = latest_filter.value.get_to_block().unwrap() + 1;

            ChaindexingRepo::update_next_block_number_to_ingest_from(
                conn,
                contract_address,
                next_block_number_to_ingest_from.as_u64() as i64,
            )
            .await
        }
    }
}
