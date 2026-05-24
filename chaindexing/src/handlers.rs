use std::cmp::min;
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
        .add_named_subtask(
            "handler-supervisor",
            tokio::spawn({
                let node_task = node_task.clone();
                let cancellation_token = node_task.cancellation_token();

                // MultiChainStates are indexed in an order-agnostic fashion, so no need for txn client
                let repo_client_for_mcs = Arc::new(Mutex::new(config.repo.get_client().await));
                let deferred_mutations_for_mcs = DeferredFutures::new();

                async move {
                    for (index, chain_ids) in get_chunked_chain_ids(&config).into_iter().enumerate()
                    {
                        let config = config.clone();
                        let repo_client_for_mcs = repo_client_for_mcs.clone();
                        let deferred_mutations_for_mcs = deferred_mutations_for_mcs.clone();
                        let cancellation_token = cancellation_token.clone();

                        node_task
                            .clone()
                            .add_named_subtask(
                                format!("handler-chain-chunk-{index}"),
                                tokio::spawn(async move {
                                    let mut interval =
                                        interval(Duration::from_millis(config.handler_rate_ms));

                                    let repo_client =
                                        Arc::new(Mutex::new(config.repo.get_client().await));
                                    let pure_handlers =
                                        contracts::get_pure_handlers(&config.contracts);
                                    let side_effect_handlers =
                                        contracts::get_side_effect_handlers(&config.contracts);

                                    loop {
                                        if cancellation_token.is_cancelled() {
                                            break;
                                        }

                                        handle_events::run(
                                            &pure_handlers,
                                            &side_effect_handlers,
                                            (&chain_ids, config.blocks_per_batch),
                                            (&repo_client, &repo_client_for_mcs),
                                            &deferred_mutations_for_mcs,
                                            &config.shared_state,
                                            config.side_effect_finality,
                                        )
                                        .await;

                                        tokio::select! {
                                            _ = interval.tick() => {}
                                            _ = cancellation_token.cancelled() => break,
                                        }
                                    }
                                }),
                            )
                            .await;
                    }

                    let mut repo_client = config.repo.get_client().await;

                    let state_migrations = contracts::get_state_migrations(&config.contracts);
                    let state_table_names = states::get_all_table_names(&state_migrations);

                    let mut interval = interval(Duration::from_millis(2 * config.handler_rate_ms));

                    loop {
                        if cancellation_token.is_cancelled() {
                            break;
                        }

                        maybe_handle_chain_reorg::run(&mut repo_client, &state_table_names).await;

                        deferred_mutations_for_mcs.consume().await;

                        tokio::select! {
                            _ = interval.tick() => {}
                            _ = cancellation_token.cancelled() => break,
                        }
                    }
                }
            }),
        )
        .await;

    node_task
}

fn get_chunked_chain_ids<S: Send + Sync + Clone + Debug + 'static>(
    config: &Config<S>,
) -> Vec<Vec<u64>> {
    let chain_ids: Vec<_> = config.chains.iter().map(|c| c.id as u64).collect();
    let worker_count = worker_count(chain_ids.len(), config.effective_handler_concurrency());

    chunk_evenly(&chain_ids, worker_count)
}

fn worker_count(item_count: usize, requested_workers: u32) -> usize {
    if item_count == 0 {
        0
    } else {
        min(item_count, requested_workers.max(1) as usize)
    }
}

fn chunk_evenly<T: Clone>(items: &[T], worker_count: usize) -> Vec<Vec<T>> {
    if worker_count == 0 {
        return vec![];
    }

    let chunk_size = items.len().div_ceil(worker_count);
    items.chunks(chunk_size).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chain, ChainId, PostgresRepo};

    fn config_with_chains(chain_count: usize) -> Config<()> {
        let mut config = Config::new(PostgresRepo::new("postgres://localhost/chaindexing"));
        for index in 0..chain_count {
            config = config.add_chain(Chain::new(
                ChainId::Mainnet,
                &format!("http://localhost:{index}"),
            ));
        }
        config
    }

    #[test]
    fn chunks_handler_chains_by_worker_cap() {
        let config = config_with_chains(7).with_handler_concurrency(3);
        let chunks = get_chunked_chain_ids(&config);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), 7);
    }

    #[test]
    fn caps_handler_workers_at_chain_count() {
        let config = config_with_chains(2).with_handler_concurrency(9);
        let chunks = get_chunked_chain_ids(&config);

        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.len() == 1));
    }
}
