use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::streams::ContractAddressesStream;
use crate::SideEffectFinality;
use crate::{contracts, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery};
use crate::{ChaindexingRepo, ChaindexingRepoClientMutex};

use super::pure_handler::{PureHandler, PureHandlerContext};
use super::side_effect_handler::{SideEffectHandler, SideEffectHandlerContext};

pub async fn run<'a, S: Send + Sync + Clone + Debug>(
    pure_handlers: &HashMap<contracts::HandlerKey, Arc<dyn PureHandler>>,
    side_effect_handlers: &HashMap<
        contracts::HandlerKey,
        Arc<dyn SideEffectHandler<SharedState = S>>,
    >,
    (chain_ids, blocks_per_batch): (&[u64], u64),
    repo_client: &ChaindexingRepoClientMutex,
    shared_state: &Option<Arc<Mutex<S>>>,
    side_effect_finality: SideEffectFinality,
) {
    for chain_id in chain_ids {
        let mut contract_addresses_stream =
            ContractAddressesStream::new(repo_client, *chain_id as i64).with_chunk_size(200);

        while let Some(contract_addresses) = contract_addresses_stream.next().await {
            for contract_address in contract_addresses {
                let from_block_number = contract_address.next_block_number_to_handle_from as u64;

                let client = repo_client.clone();
                let mut client = client.lock().await;

                // return ordered by block_number and log_index
                let events = ChaindexingRepo::load_events(
                    &client,
                    *chain_id,
                    &contract_address.address,
                    from_block_number,
                    blocks_per_batch,
                )
                .await;

                // ChainStates which include ContractState have to be handled orderly
                let txn_client = ChaindexingRepo::get_txn_client(&mut client).await;

                for event in &events {
                    let handler_key = contracts::handler_key(&event.contract_name, event.get_abi());
                    let is_at_block_tail = event_is_at_block_tail(
                        event.get_block_number(),
                        contract_address.next_block_number_to_ingest_from,
                    );

                    {
                        if let Some(handler) = pure_handlers.get(&handler_key) {
                            let handler_context = PureHandlerContext::from_txn(event, &txn_client)
                                .with_is_at_block_tail(is_at_block_tail);

                            handler.handle_event(handler_context).await;
                        }
                    }

                    {
                        if event.block_number >= contract_address.next_block_number_for_side_effects
                        {
                            if let Some(handler) = side_effect_handlers.get(&handler_key) {
                                let handler_context = SideEffectHandlerContext::new(
                                    event,
                                    &txn_client,
                                    shared_state,
                                    side_effect_finality,
                                )
                                .with_is_at_block_tail(is_at_block_tail);

                                handler.handle_event(handler_context).await;
                            }
                        }
                    }
                }

                if let Some(last_event) = events.last() {
                    let next_block_number_to_handle_from = last_event.block_number as u64 + 1;

                    ChaindexingRepo::update_next_block_number_to_handle_from(
                        &txn_client,
                        &contract_address.address,
                        *chain_id,
                        next_block_number_to_handle_from,
                    )
                    .await;

                    if next_block_number_to_handle_from
                        > contract_address.next_block_number_for_side_effects as u64
                    {
                        ChaindexingRepo::update_next_block_number_for_side_effects(
                            &txn_client,
                            &contract_address.address,
                            *chain_id,
                            next_block_number_to_handle_from,
                        )
                        .await;
                    }
                }

                ChaindexingRepo::commit_txns(txn_client).await;
            }
        }
    }
}

fn event_is_at_block_tail(event_block_number: u64, next_block_number_to_ingest_from: i64) -> bool {
    let next_block_number_to_ingest_from = next_block_number_to_ingest_from.max(0) as u64;

    event_block_number.saturating_add(1) >= next_block_number_to_ingest_from
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_before_ingested_tail_is_not_at_block_tail() {
        assert!(!event_is_at_block_tail(10, 12));
    }

    #[test]
    fn event_at_latest_ingested_block_is_at_block_tail() {
        assert!(event_is_at_block_tail(11, 12));
    }

    #[test]
    fn event_past_cursor_is_treated_as_tail_after_rewinds() {
        assert!(event_is_at_block_tail(12, 12));
    }
}
