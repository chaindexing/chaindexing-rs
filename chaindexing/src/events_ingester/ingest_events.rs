use std::collections::HashMap;
use std::sync::Arc;

use futures_util::FutureExt;

use crate::chain_reorg::Execution;
use crate::contracts::Contract;
use crate::events::Events;
use crate::{
    ChaindexingRepo, ChaindexingRepoConn, ChaindexingRepoRawQueryClient, ContractAddress,
    EventsIngesterJsonRpc, LoadsDataWithRawQuery, Repo,
};

use super::{fetch_blocks_by_number, fetch_logs, EventsIngesterError, Filter, Filters};

pub async fn run<'a, S: Send + Sync + Clone>(
    conn: &mut ChaindexingRepoConn<'a>,
    raw_query_client: &ChaindexingRepoRawQueryClient,
    contract_addresses: Vec<ContractAddress>,
    contracts: &Vec<Contract<S>>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
    current_block_number: u64,
    blocks_per_batch: u64,
) -> Result<(), EventsIngesterError> {
    let filters = Filters::get(
        &contract_addresses,
        contracts,
        current_block_number,
        blocks_per_batch,
        &Execution::Main,
    );

    let filters =
        remove_already_ingested_filters(&filters, &contract_addresses, raw_query_client).await;

    if !filters.is_empty() {
        let logs = fetch_logs(&filters, json_rpc).await;
        let blocks_by_tx_hash = fetch_blocks_by_number(&logs, json_rpc).await;
        let events = Events::get(&logs, contracts, &blocks_by_tx_hash);

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
    raw_query_client: &ChaindexingRepoRawQueryClient,
) -> Vec<Filter> {
    let current_block_filters: Vec<_> = filters
        .iter()
        .filter(|f| f.value.get_from_block() == f.value.get_to_block())
        .collect();

    if current_block_filters.is_empty() {
        filters.to_owned()
    } else {
        let addresses = contract_addresses.iter().map(|c| c.address.clone()).collect();

        let latest_ingested_events =
            ChaindexingRepo::load_latest_events(raw_query_client, &addresses).await;
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
    contract_addresses: &Vec<ContractAddress>,
    filters: &Vec<Filter>,
) {
    let filters_by_contract_address_id = Filters::group_by_contract_address_id(filters);

    for contract_address in contract_addresses {
        let filters = filters_by_contract_address_id.get(&contract_address.id).unwrap();

        if let Some(latest_filter) = Filters::get_latest(filters) {
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
