use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::ChaindexingRepo;
use crate::{
    handler_subscriptions, ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, HasRawQueryClient,
    LoadsDataWithRawQuery,
};

use super::pure_handler::{PureHandler, PureHandlerContext};
use super::side_effect_handler::{SideEffectHandler, SideEffectHandlerContext};

pub async fn run<'a, S: Send + Sync + Clone + Debug>(
    pure_handlers_by_event_abi: &HashMap<&str, Arc<dyn PureHandler>>,
    side_effect_handlers_by_event_abi: &HashMap<&str, Arc<dyn SideEffectHandler<SharedState = S>>>,
    (chain_ids, blocks_per_batch): (&[u64], u64),
    raw_query_client_for_cs: &mut ChaindexingRepoRawQueryClient,
    raw_query_client_for_mcs: &Arc<Mutex<ChaindexingRepoRawQueryClient>>,
    deferred_mutations_for_mcs: DeferredFutures<'a>,
    shared_state: &Option<Arc<Mutex<S>>>,
) {
    let subscriptions =
        ChaindexingRepo::load_handler_subscriptions(raw_query_client_for_cs, chain_ids).await;
    let subscriptions_by_chain_id = handler_subscriptions::group_by_chain_id(&subscriptions);

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

    for events in events.chunk_by(|e1, e2| e1.chain_id == e2.chain_id).filter(|c| !c.is_empty()) {
        let last_event = events.last().unwrap();

        let chain_id = last_event.chain_id as u64;

        let subscription = subscriptions_by_chain_id.get(&chain_id).unwrap();

        for event in events
            .iter()
            .filter(|e| e.block_number as u64 >= subscription.next_block_number_to_handle_from)
        {
            {
                let handler = pure_handlers_by_event_abi.get(event.abi.as_str()).unwrap();
                let handler_context = PureHandlerContext::new(
                    event,
                    &raw_query_txn_client_for_cs,
                    raw_query_client_for_mcs,
                    &deferred_mutations_for_mcs,
                );

                handler.handle_event(handler_context).await;
            }

            {
                if event.block_number as u64 >= subscription.next_block_number_for_side_effects {
                    if let Some(handler) = side_effect_handlers_by_event_abi.get(event.abi.as_str())
                    {
                        let handler_context = SideEffectHandlerContext::new(
                            event,
                            &raw_query_txn_client_for_cs,
                            shared_state,
                        );

                        handler.handle_event(handler_context).await;
                    }
                }
            }
        }

        let next_block_number_to_handle_from = last_event.block_number as u64 + 1;

        ChaindexingRepo::update_handler_subscription_next_block_number_to_handle_from(
            &raw_query_txn_client_for_cs,
            chain_id,
            next_block_number_to_handle_from,
        )
        .await;

        if next_block_number_to_handle_from > subscription.next_block_number_for_side_effects {
            ChaindexingRepo::update_handler_subscription_next_block_number_for_side_effects(
                &raw_query_txn_client_for_cs,
                chain_id,
                next_block_number_to_handle_from,
            )
            .await;
        }
    }

    ChaindexingRepo::commit_raw_query_txns(raw_query_txn_client_for_cs).await;
}
