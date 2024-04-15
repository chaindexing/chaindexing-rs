use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::streams::ContractAddressesStream;
use crate::{ChaindexingRepo, ChaindexingRepoClientMutex};
use crate::{EventAbi, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery};

use super::pure_handler::{PureHandler, PureHandlerContext};
use super::side_effect_handler::{SideEffectHandler, SideEffectHandlerContext};

pub async fn run<'a, S: Send + Sync + Clone + Debug>(
    pure_handlers: &HashMap<EventAbi, Arc<dyn PureHandler>>,
    side_effect_handlers: &HashMap<EventAbi, Arc<dyn SideEffectHandler<SharedState = S>>>,
    (chain_ids, blocks_per_batch): (&[u64], u64),
    (repo_client, repo_client_for_mcs): (&ChaindexingRepoClientMutex, &ChaindexingRepoClientMutex),
    deferred_mutations_for_mcs: &DeferredFutures<'a>,
    shared_state: &Option<Arc<Mutex<S>>>,
) {
    for chain_id in chain_ids {
        let mut contract_addresses_stream =
            ContractAddressesStream::new(repo_client, *chain_id as i64).with_chunk_size(200);

        while let Some(contract_addresses) = contract_addresses_stream.next().await {
            for contract_address in contract_addresses {
                let from_block_number = contract_address.next_block_number_to_handle_from as u64;
                let to_block_number = from_block_number + blocks_per_batch;

                let client = repo_client.clone();
                let mut client = client.lock().await;

                // return ordered by block_number and log_index
                let events = ChaindexingRepo::load_events(
                    &client,
                    *chain_id,
                    &contract_address.address,
                    from_block_number,
                    to_block_number,
                )
                .await;

                // ChainStates which include ContractState have to be handled orderly
                let txn_client = ChaindexingRepo::get_txn_client(&mut client).await;

                for event in &events {
                    {
                        let handler = pure_handlers.get(event.get_abi()).unwrap();
                        let handler_context = PureHandlerContext::new(
                            event,
                            &txn_client,
                            repo_client_for_mcs,
                            deferred_mutations_for_mcs,
                        );

                        handler.handle_event(handler_context).await;
                    }

                    {
                        if event.block_number >= contract_address.next_block_number_for_side_effects
                        {
                            if let Some(handler) = side_effect_handlers.get(event.get_abi()) {
                                let handler_context =
                                    SideEffectHandlerContext::new(event, &txn_client, shared_state);

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
