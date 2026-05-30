use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::rpc::types::Block;
use futures_util::FutureExt;
use std::cmp::min;
use tokio::sync::Mutex;

use crate::chain_blocks::{self, ChainBlock};
use crate::chain_reorg::{Execution, UnsavedReorgedBlock};
use crate::events::{self, Event};
use crate::Config;
use crate::{
    ChainId, ChaindexingRepo, ChaindexingRepoConn, ContractAddress, IndexedDataConfig, Repo,
    RepoError,
};

use super::block_logs;
use super::filters::{self, Filter};
use super::indexed_data_capture::{self, FetchedIndexedData};
use super::Provider;
use super::{provider, IngesterError};

pub async fn run<'a, S: Send + Sync + Clone>(
    conn: &Arc<Mutex<ChaindexingRepoConn<'a>>>,
    contract_addresses: Vec<ContractAddress>,
    provider: &Arc<impl Provider>,
    chain_id: &ChainId,
    current_block_number: u64,
    config @ Config {
        contracts,
        min_confirmation_count,
        blocks_per_batch,
        ..
    }: &Config<S>,
) -> Result<(), IngesterError> {
    let filters = filters::get(
        &contract_addresses,
        contracts,
        current_block_number,
        *blocks_per_batch,
        &Execution::Confirmation(min_confirmation_count),
    );

    if !filters.is_empty() {
        let already_ingested_events = {
            let mut conn = conn.lock().await;
            get_already_ingested_events(&mut conn, chain_id, &filters).await?
        };
        let rpc = config.runtime_config.rpc_ref();
        let blocks_by_number = provider::fetch_blocks_for_filters_with_policy(
            provider,
            &filters,
            current_block_number,
            min_confirmation_count.as_u64(),
            provider::FetchPolicy::from_rpc_policy(rpc),
        )
        .await?;
        let block_logs = block_logs::fetch(
            provider,
            &filters,
            chain_id,
            &blocks_by_number,
            provider::FetchPolicy::from_rpc_policy(rpc),
        )
        .await?;
        let chain_blocks = chain_blocks::from_provider_blocks(chain_id, &blocks_by_number);

        let provider_events = events::get(
            &block_logs.logs,
            contracts,
            &contract_addresses,
            chain_id,
            &blocks_by_number,
        );

        let added_and_removed_events =
            get_provider_added_and_removed_events(&already_ingested_events, &provider_events);

        let block_fork_point = if config.indexed_data_config.enabled() && !chain_blocks.is_empty() {
            let mut conn = conn.lock().await;
            ChaindexingRepo::find_fork_point(&mut conn, &chain_blocks).await?
        } else {
            None
        };
        let indexed_data = if let Some(fork_point) = block_fork_point {
            fetch_indexed_data_for_replacement_blocks(
                provider,
                ReplacementIndexedDataRequest {
                    chain_id,
                    filters: &filters,
                    current_block_number,
                    lookback_block_count: min_confirmation_count.as_u64(),
                    blocks_by_number: &blocks_by_number,
                    fork_point,
                    indexed_data_config: &config.indexed_data_config,
                    policy: provider::FetchPolicy::from_rpc_policy(rpc),
                },
            )
            .await?
        } else {
            FetchedIndexedData::empty()
        };

        if !chain_blocks.is_empty() || added_and_removed_events.is_some() {
            let mut conn = conn.lock().await;
            handle_chain_reorg(
                &mut conn,
                chain_id,
                chain_blocks,
                block_logs.scans,
                added_and_removed_events,
                indexed_data,
            )
            .await?;
        }
    }

    Ok(())
}

async fn get_already_ingested_events<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    chain_id: &ChainId,
    filters: &[Filter],
) -> Result<Vec<Event>, RepoError> {
    let mut already_ingested_events = vec![];
    for filter in filters {
        let from_block = filter.value.get_from_block().unwrap();
        let to_block = filter.value.get_to_block().unwrap();

        let events = ChaindexingRepo::get_events(
            conn,
            *chain_id as u64,
            filter.address.to_owned(),
            from_block,
            to_block,
        )
        .await;
        let mut events = events?;
        already_ingested_events.append(&mut events);
    }

    Ok(already_ingested_events)
}

async fn handle_chain_reorg<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    chain_id: &ChainId,
    chain_blocks: Vec<ChainBlock>,
    block_scans: Vec<chain_blocks::BlockScan>,
    added_and_removed_events: Option<(Vec<Event>, Vec<Event>)>,
    indexed_data: FetchedIndexedData,
) -> Result<(), IngesterError> {
    let chain_id = *chain_id;

    ChaindexingRepo::run_in_transaction(conn, move |conn| {
        async move {
            let block_reorg_number =
                ChaindexingRepo::sync_blocks(conn, &chain_id, &chain_blocks).await?;
            let event_reorg_number =
                added_and_removed_events.as_ref().map(|(added_events, removed_events)| {
                    get_earliest_block_number(added_events, removed_events)
                });

            let earliest_block_number = match (block_reorg_number, event_reorg_number) {
                (Some(block_reorg_number), Some(event_reorg_number)) => {
                    min(block_reorg_number, event_reorg_number)
                }
                (Some(block_reorg_number), None) => block_reorg_number,
                (None, Some(event_reorg_number)) => event_reorg_number,
                (None, None) => return Ok(()),
            };

            let new_reorged_block = UnsavedReorgedBlock::new(earliest_block_number, &chain_id);
            ChaindexingRepo::create_reorged_block(conn, &new_reorged_block).await?;

            let (added_events, removed_events) = added_and_removed_events.unwrap_or_default();
            if block_reorg_number.is_some() {
                ChaindexingRepo::delete_events_from_block_number(
                    conn,
                    &chain_id,
                    earliest_block_number,
                )
                .await?;
            } else {
                let event_ids: Vec<_> = removed_events.iter().map(|e| e.id).collect();
                ChaindexingRepo::delete_events_by_ids(conn, &event_ids).await?;
            }

            ChaindexingRepo::create_block_scans(conn, &block_scans).await?;
            ChaindexingRepo::create_transactions(conn, &indexed_data.transactions).await?;
            ChaindexingRepo::create_call_traces(conn, &indexed_data.call_traces).await?;
            ChaindexingRepo::create_events(conn, &added_events).await?;

            Ok(())
        }
        .boxed()
    })
    .await?;

    Ok(())
}

fn get_provider_added_and_removed_events(
    already_ingested_events: &[Event],
    provider_events: &[Event],
) -> Option<(Vec<Event>, Vec<Event>)> {
    let already_ingested_events_set: HashSet<_> = already_ingested_events.iter().cloned().collect();
    let provider_events_set: HashSet<_> = provider_events.iter().cloned().collect();

    let added_events: Vec<_> = provider_events
        .iter()
        .filter(|e| !already_ingested_events_set.contains(e))
        .cloned()
        .collect();

    let removed_events: Vec<_> = already_ingested_events
        .iter()
        .filter(|e| !provider_events_set.contains(e))
        .cloned()
        .collect();

    if added_events.is_empty() && removed_events.is_empty() {
        None
    } else {
        Some((added_events, removed_events))
    }
}

struct ReplacementIndexedDataRequest<'a> {
    chain_id: &'a ChainId,
    filters: &'a [Filter],
    current_block_number: u64,
    lookback_block_count: u64,
    blocks_by_number: &'a HashMap<u64, Block>,
    fork_point: i64,
    indexed_data_config: &'a IndexedDataConfig,
    policy: provider::FetchPolicy,
}

async fn fetch_indexed_data_for_replacement_blocks(
    provider: &Arc<impl Provider>,
    request: ReplacementIndexedDataRequest<'_>,
) -> Result<FetchedIndexedData, IngesterError> {
    if request.indexed_data_config.requires_full_blocks() {
        let full_blocks_by_number = provider::fetch_full_blocks_for_filters_with_policy(
            provider,
            request.filters,
            request.current_block_number,
            request.lookback_block_count,
            request.policy,
        )
        .await?;
        let replacement_blocks = indexed_data_capture::blocks_from_fork_point(
            &full_blocks_by_number,
            request.fork_point,
        );

        return indexed_data_capture::fetch_for_blocks(
            provider,
            request.chain_id,
            &replacement_blocks,
            request.indexed_data_config,
            request.policy,
        )
        .await;
    }

    let replacement_blocks =
        indexed_data_capture::blocks_from_fork_point(request.blocks_by_number, request.fork_point);

    indexed_data_capture::fetch_for_blocks(
        provider,
        request.chain_id,
        &replacement_blocks,
        request.indexed_data_config,
        request.policy,
    )
    .await
}

fn get_earliest_block_number(added_events: &[Event], removed_events: &[Event]) -> i64 {
    let earliest_added_event = added_events.iter().min_by_key(|e| e.block_number);
    let earliest_removed_event = removed_events.iter().min_by_key(|e| e.block_number);

    match (earliest_added_event, earliest_removed_event) {
        (Some(event), None) | (None, Some(event)) => event.block_number,
        (Some(earliest_added), Some(earliest_removed)) => {
            min(earliest_added.block_number, earliest_removed.block_number)
        }
        _ => unreachable!("Added Events or Removed Events must have at least one entry"),
    }
}
