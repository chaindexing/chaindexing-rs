use std::collections::HashMap;
use std::sync::Arc;

use futures_util::FutureExt;

use super::block_logs;
use super::filters::{self, Filter};
use super::provider::{self, Provider};
use super::IngesterError;

use crate::chain_blocks;
use crate::chain_reorg::{Execution, UnsavedReorgedBlock};
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
    config @ Config {
        contracts,
        blocks_per_batch,
        min_confirmation_count,
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

    let filters =
        remove_already_ingested_filters(&filters, &contract_addresses, chain_id, repo_client).await;

    if !filters.is_empty() {
        let rpc = config.runtime_config.rpc_ref();
        let blocks_by_tx_hash = provider::fetch_blocks_for_filters_with_policy(
            provider,
            &filters,
            current_block_number,
            min_confirmation_count.as_u64(),
            rpc.retry_attempts_value(),
            rpc.base_backoff_ms_value(),
            rpc.max_backoff_ms_value(),
        )
        .await?;
        let block_logs = block_logs::fetch(
            provider,
            &filters,
            chain_id,
            &blocks_by_tx_hash,
            block_logs::FetchPolicy::from_rpc_policy(rpc),
        )
        .await?;
        let chain_blocks = chain_blocks::from_provider_blocks(chain_id, &blocks_by_tx_hash);
        let events = events::get(
            &block_logs.logs,
            contracts,
            &contract_addresses,
            chain_id,
            &blocks_by_tx_hash,
        );
        let block_scans = block_logs.scans;
        let contract_addresses = contract_addresses.clone();
        let chain_id = *chain_id;

        ChaindexingRepo::run_in_transaction(conn, move |conn| {
            async move {
                if let Some(block_number) =
                    ChaindexingRepo::sync_blocks(conn, &chain_id, &chain_blocks).await
                {
                    let reorged_block = UnsavedReorgedBlock::new(block_number, &chain_id);
                    ChaindexingRepo::create_reorged_block(conn, &reorged_block).await;
                    ChaindexingRepo::delete_events_from_block_number(conn, &chain_id, block_number)
                        .await;
                    rewind_next_block_numbers_to_ingest_from(
                        conn,
                        &contract_addresses,
                        block_number,
                    )
                    .await;

                    return Ok(());
                }

                ChaindexingRepo::create_block_scans(conn, &block_scans).await;
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
    filters: &[Filter],
    contract_addresses: &[ContractAddress],
    chain_id: &ChainId,
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
            ChaindexingRepo::load_latest_events(repo_client, *chain_id as u64, &addresses).await;
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

async fn rewind_next_block_numbers_to_ingest_from<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    contract_addresses: &[ContractAddress],
    block_number: i64,
) {
    for contract_address in contract_addresses {
        ChaindexingRepo::update_next_block_number_to_ingest_from(
            conn,
            contract_address,
            block_number,
        )
        .await
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
