use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::{events::Event, ChaindexingRepo};
use crate::{
    ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery,
};

use super::{EventHandler, EventHandlerContext};

pub async fn run<'a, S: Send + Sync + Clone + Debug>(
    event_handlers_by_event_abi: &HashMap<&str, Arc<dyn EventHandler<SharedState = S>>>,
    chain_ids: &[u64],
    blocks_per_batch: u64,
    raw_query_client_for_cs: &mut ChaindexingRepoRawQueryClient,
    raw_query_client_for_mcs: &Arc<Mutex<ChaindexingRepoRawQueryClient>>,
    deferred_mutations_for_mcs: DeferredFutures<'a>,
    shared_state: &Option<Arc<Mutex<S>>>,
) {
    let subscriptions =
        ChaindexingRepo::load_event_handler_subscriptions(raw_query_client_for_cs, chain_ids).await;

    let from_block_number = subscriptions
        .iter()
        .min_by_key(|es| es.next_block_number_to_handle_from)
        .map(|es| es.next_block_number_to_handle_from)
        .unwrap_or(0);
    let to_block_number = from_block_number + blocks_per_batch;

    // return ordered by block_number and log_index
    let events = ChaindexingRepo::load_events(
        raw_query_client_for_cs,
        chain_ids,
        from_block_number,
        to_block_number,
    )
    .await;

    // ChainStates which include ContractState have to be handled orderly
    let raw_query_txn_client_for_cs =
        ChaindexingRepo::get_raw_query_txn_client(raw_query_client_for_cs).await;

    for events in events.chunk_by(|e1, e2| e1.chain_id == e2.chain_id) {
        for event in events {
            let event_handler = event_handlers_by_event_abi.get(event.abi.as_str()).unwrap();
            let event_handler_context = EventHandlerContext::new(
                event,
                &raw_query_txn_client_for_cs,
                raw_query_client_for_mcs,
                &deferred_mutations_for_mcs,
                shared_state,
            );

            event_handler.handle_event(event_handler_context).await;
        }

        if let Some(Event { block_number, .. }) = events.last() {
            let next_block_number_to_handle_from = *block_number as u64 + 1;
            ChaindexingRepo::update_event_handler_subscriptions_next_block(
                &raw_query_txn_client_for_cs,
                chain_ids,
                next_block_number_to_handle_from,
            )
            .await;
        }
    }

    ChaindexingRepo::commit_raw_query_txns(raw_query_txn_client_for_cs).await;
}
