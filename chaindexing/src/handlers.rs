use std::cmp::max;
use std::fmt::Debug;
use std::{sync::Arc, time::Duration};

mod handle_events;
mod handler_context;
mod maybe_handle_chain_reorg;
mod pure_handler;
mod side_effect_handler;

pub use handler_context::HandlerContext;
pub use pure_handler::{PureHandler, PureHandlerContext};
pub use side_effect_handler::{SideEffectHandler, SideEffectHandlerContext};

use tokio::{sync::Mutex, time::interval};

use crate::deferred_futures::DeferredFutures;
use crate::nodes::NodeTask;
use crate::Config;
use crate::{contracts, states, HasRawQueryClient};

// TODO: Use just raw query client through for mutations
pub async fn start<S: Send + Sync + Clone + Debug + 'static>(config: &Config<S>) -> NodeTask {
    let node_task = NodeTask::new();
    let config = config.clone();

    node_task
        .add_task(tokio::spawn({
            let node_task = node_task.clone();

            // MultiChainStates are indexed in an order-agnostic fashion, so no need for txn client
            let raw_query_client_for_mcs = config.repo.get_raw_query_client().await;
            let raw_query_client_for_mcs = Arc::new(Mutex::new(raw_query_client_for_mcs));

            let deferred_mutations_for_mcs = DeferredFutures::new();

            let chain_ids: Vec<_> = config.chains.iter().map(|c| c.id as u64).collect();
            let chain_ids_count = chain_ids.len();
            let chunk_size = max(chain_ids_count / config.chain_concurrency as usize, 1);
            let chunked_chain_ids: Vec<_> =
                chain_ids.chunks(chunk_size).map(|c| c.to_vec()).collect();

            async move {
                for chain_ids in chunked_chain_ids {
                    let config = config.clone();
                    let node_task = node_task.clone();

                    let raw_query_client_for_mcs = raw_query_client_for_mcs.clone();
                    let deferred_mutations_for_mcs = deferred_mutations_for_mcs.clone();

                    node_task
                        .clone()
                        .add_task(tokio::spawn(async move {
                            // ChainStates which include ContractState have to be handled orderly
                            let mut raw_query_client_for_chain_states =
                                config.repo.get_raw_query_client().await;

                            let mut interval =
                                interval(Duration::from_millis(config.handler_rate_ms));
                            let pure_handlers_by_event_abi =
                                contracts::get_pure_handlers_by_event_abi(&config.contracts);
                            let side_effect_handlers_by_event_abi =
                                contracts::get_side_effect_handlers_by_event_abi(&config.contracts);

                            loop {
                                handle_events::run(
                                    &pure_handlers_by_event_abi,
                                    &side_effect_handlers_by_event_abi,
                                    (&chain_ids, config.blocks_per_batch),
                                    &mut raw_query_client_for_chain_states,
                                    &raw_query_client_for_mcs,
                                    deferred_mutations_for_mcs.clone(),
                                    &config.shared_state,
                                )
                                .await;

                                interval.tick().await;
                            }
                        }))
                        .await;
                }

                let mut raw_query_client_for_chain_states =
                    config.repo.get_raw_query_client().await;

                let state_migrations = contracts::get_state_migrations(&config.contracts);
                let state_table_names = states::get_all_table_names(&state_migrations);

                let mut interval = interval(Duration::from_millis(
                    chain_ids_count as u64 * config.handler_rate_ms,
                ));

                loop {
                    maybe_handle_chain_reorg::run(
                        &mut raw_query_client_for_chain_states,
                        &state_table_names,
                    )
                    .await;

                    deferred_mutations_for_mcs.consume().await;

                    interval.tick().await;
                }
            }
        }))
        .await;

    node_task
}
