use std::collections::HashSet;
use std::sync::Arc;

use ethers::prelude::*;
use futures_util::FutureExt;
use std::cmp::min;

use crate::chain_reorg::{Execution, UnsavedReorgedBlock};
use crate::contracts::Contract;
use crate::events::{Event, Events};
use crate::{
    ChaindexingRepo, ChaindexingRepoConn, ContractAddress, EventsIngesterJsonRpc,
    MinConfirmationCount, Repo,
};

use super::{fetch_blocks_by_number, fetch_logs, EventsIngesterError, Filter, Filters};

pub async fn run<'a, S: Send + Sync + Clone>(
    conn: &mut ChaindexingRepoConn<'a>,
    contract_addresses: Vec<ContractAddress>,
    contracts: &Vec<Contract<S>>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
    chain: &Chain,
    current_block_number: u64,
    blocks_per_batch: u64,
    min_confirmation_count: &MinConfirmationCount,
) -> Result<(), EventsIngesterError> {
    let filters = Filters::get(
        &contract_addresses,
        contracts,
        current_block_number,
        blocks_per_batch,
        &Execution::Confirmation(min_confirmation_count),
    );

    if !filters.is_empty() {
        let already_ingested_events = get_already_ingested_events(conn, &filters).await;
        let json_rpc_events = get_json_rpc_events(&filters, json_rpc, contracts).await;

        if let Some(added_and_removed_events) =
            get_json_rpc_added_and_removed_events(&already_ingested_events, &json_rpc_events)
        {
            handle_chain_reorg(conn, chain, added_and_removed_events).await?;
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

async fn get_json_rpc_events<S: Send + Sync + Clone>(
    filters: &Vec<Filter>,
    json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
    contracts: &Vec<Contract<S>>,
) -> Vec<Event> {
    let logs = fetch_logs(filters, json_rpc).await;
    let blocks_by_number = fetch_blocks_by_number(&logs, json_rpc).await;

    Events::get(&logs, contracts, &blocks_by_number)
}

async fn handle_chain_reorg<'a>(
    conn: &mut ChaindexingRepoConn<'a>,
    chain: &Chain,
    (added_events, removed_events): (Vec<Event>, Vec<Event>),
) -> Result<(), EventsIngesterError> {
    let earliest_block_number = get_earliest_block_number((&added_events, &removed_events));
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

    Ok(())
}

fn get_json_rpc_added_and_removed_events(
    already_ingested_events: &Vec<Event>,
    json_rpc_events: &Vec<Event>,
) -> Option<(Vec<Event>, Vec<Event>)> {
    let already_ingested_events_set: HashSet<_> = already_ingested_events.iter().cloned().collect();
    let json_rpc_events_set: HashSet<_> = json_rpc_events.iter().cloned().collect();

    let added_events: Vec<_> = json_rpc_events
        .iter()
        .filter(|e| !already_ingested_events_set.contains(e))
        .cloned()
        .collect();

    let removed_events: Vec<_> = already_ingested_events
        .iter()
        .filter(|e| !json_rpc_events_set.contains(e))
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
        (None, Some(event)) => event.block_number,
        (Some(event), None) => event.block_number,
        (Some(earliest_added), Some(earliest_removed)) => {
            min(earliest_added.block_number, earliest_removed.block_number)
        }
        _ => unreachable!("Added Events or Removed Events must have at least one entry"),
    }
}
