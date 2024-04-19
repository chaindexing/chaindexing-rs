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

pub async fn start<S: Send + Sync + Clone + Debug + 'static>(config: &Config<S>) -> NodeTask {
    let node_task = NodeTask::new();
    let config = config.clone();

    node_task
        .add_subtask(tokio::spawn({
            let node_task = node_task.clone();

            // MultiChainStates are indexed in an order-agnostic fashion, so no need for txn client
            let repo_client_for_mcs = Arc::new(Mutex::new(config.repo.get_client().await));
            let deferred_mutations_for_mcs = DeferredFutures::new();

            async move {
                for chain_ids in get_chunked_chain_ids(&config) {
                    let config = config.clone();
                    let repo_client_for_mcs = repo_client_for_mcs.clone();
                    let deferred_mutations_for_mcs = deferred_mutations_for_mcs.clone();

                    node_task
                        .clone()
                        .add_subtask(tokio::spawn(async move {
                            let mut interval =
                                interval(Duration::from_millis(config.handler_rate_ms));

                            let repo_client = Arc::new(Mutex::new(config.repo.get_client().await));
                            let pure_handlers = contracts::get_pure_handlers(&config.contracts);
                            let side_effect_handlers =
                                contracts::get_side_effect_handlers(&config.contracts);

                            loop {
                                handle_events::run(
                                    &pure_handlers,
                                    &side_effect_handlers,
                                    (&chain_ids, config.blocks_per_batch),
                                    (&repo_client, &repo_client_for_mcs),
                                    &deferred_mutations_for_mcs,
                                    &config.shared_state,
                                )
                                .await;

                                interval.tick().await;
                            }
                        }))
                        .await;
                }

                let mut repo_client = config.repo.get_client().await;

                let state_migrations = contracts::get_state_migrations(&config.contracts);
                let state_table_names = states::get_all_table_names(&state_migrations);

                let mut interval = interval(Duration::from_millis(2 * config.handler_rate_ms));

                loop {
                    maybe_handle_chain_reorg::run(&mut repo_client, &state_table_names).await;

                    deferred_mutations_for_mcs.consume().await;

                    interval.tick().await;
                }
            }
        }))
        .await;

    node_task
}

fn get_chunked_chain_ids<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Vec<Vec<u64>> {
    let chain_ids: Vec<_> = config.chains.iter().map(|c| c.id as u64).collect();
    let chain_ids_count = chain_ids.len();
    let chunk_size = max(chain_ids_count / config.chain_concurrency as usize, 1);

    chain_ids.chunks(chunk_size).map(|c| c.to_vec()).collect()
}
