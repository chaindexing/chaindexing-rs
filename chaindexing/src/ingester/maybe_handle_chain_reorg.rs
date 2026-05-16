use std::collections::HashSet;
use std::sync::Arc;

use futures_util::FutureExt;
use std::cmp::min;

use crate::chain_blocks::{self, ChainBlock};
use crate::chain_reorg::{Execution, UnsavedReorgedBlock};
use crate::events::{self, Event};
use crate::Config;
use crate::{ChainId, ChaindexingRepo, ChaindexingRepoConn, ContractAddress, Repo};

use super::filters::{self, Filter};
use super::Provider;
use super::{provider, IngesterError};

pub async fn run<'a, S: Send + Sync + Clone>(
    conn: &mut ChaindexingRepoConn<'a>,
    contract_addresses: Vec<ContractAddress>,
    provider: &Arc<impl Provider>,
    chain_id: &ChainId,
    current_block_number: u64,
    Config {
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
        let already_ingested_events = get_already_ingested_events(conn, &filters).await;
        let logs = provider::fetch_logs(provider, &filters).await;
        let blocks_by_number = provider::fetch_blocks_by_number(provider, &logs).await;
        let chain_blocks = chain_blocks::from_provider_blocks(chain_id, &blocks_by_number);

        let provider_events = events::get(
            &logs,
            contracts,
            &contract_addresses,
            chain_id,
            &blocks_by_number,
        );

        let added_and_removed_events =
            get_provider_added_and_removed_events(&already_ingested_events, &provider_events);

        if !chain_blocks.is_empty() || added_and_removed_events.is_some() {
            handle_chain_reorg(conn, chain_id, chain_blocks, added_and_removed_events).await?;
        }
    }

    Ok(())
}

async fn get_already_ingested_events<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    filters: &Vec<Filter>,
) -> Vec<Event> {
    let mut already_ingested_events = vec![];
    for filter in filters {
        let from_block = filter.value.get_from_block().unwrap().as_u64();
        let to_block = filter.value.get_to_block().unwrap().as_u64();

        let mut events =
            ChaindexingRepo::get_events(conn, filter.address.to_owned(), from_block, to_block)
                .await;
        already_ingested_events.append(&mut events);
    }

    already_ingested_events
}

async fn handle_chain_reorg<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    chain_id: &ChainId,
    chain_blocks: Vec<ChainBlock>,
    added_and_removed_events: Option<(Vec<Event>, Vec<Event>)>,
) -> Result<(), IngesterError> {
    let chain_id = *chain_id;

    ChaindexingRepo::run_in_transaction(conn, move |conn| {
        async move {
            let block_reorg_number =
                ChaindexingRepo::sync_blocks(conn, &chain_id, &chain_blocks).await;
            let event_reorg_number =
                added_and_removed_events.as_ref().map(|(added_events, removed_events)| {
                    get_earliest_block_number((added_events, removed_events))
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
            ChaindexingRepo::create_reorged_block(conn, &new_reorged_block).await;

            let (added_events, removed_events) = added_and_removed_events.unwrap_or_default();
            if block_reorg_number.is_some() {
                ChaindexingRepo::delete_events_from_block_number(
                    conn,
                    &chain_id,
                    earliest_block_number,
                )
                .await;
            } else {
                let event_ids: Vec<_> = removed_events.iter().map(|e| e.id).collect();
                ChaindexingRepo::delete_events_by_ids(conn, &event_ids).await;
            }

            ChaindexingRepo::create_events(conn, &added_events).await;

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

fn get_earliest_block_number((added_events, removed_events): (&Vec<Event>, &Vec<Event>)) -> i64 {
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
