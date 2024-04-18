mod error;
mod filters;
mod ingest_events;
mod maybe_handle_chain_reorg;
mod provider;

pub use error::IngesterError;
pub use provider::{Provider, ProviderError};

use std::cmp::max;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::contracts;
use crate::nodes::NodeTask;
use crate::pruning::PruningConfig;
use crate::states;
use crate::streams::ContractAddressesStream;
use crate::Chain;
use crate::ChainId;
use crate::Config;
use crate::Contract;
use crate::ContractAddress;
use crate::{ChaindexingRepo, ChaindexingRepoClient, ChaindexingRepoConn};
use crate::{ExecutesWithRawQuery, HasRawQueryClient, Repo};

pub async fn start<S: Sync + Send + Clone + 'static>(config: &Config<S>) -> NodeTask {
    let node_task = NodeTask::new();

    for chains in get_chunked_chains(config) {
        let config = config.clone();

        node_task
            .add_subtask(tokio::spawn(async move {
                let mut interval = interval(Duration::from_millis(config.ingestion_rate_ms));
                let mut last_pruned_at_per_chain_id = HashMap::new();

                loop {
                    for chain in chains.iter() {
                        let provider = provider::get(&chain.json_rpc_url);
                        let repo_client = Arc::new(Mutex::new(config.repo.get_client().await));
                        let pool = config.repo.get_pool(1).await;
                        let conn = ChaindexingRepo::get_conn(&pool).await;
                        let conn = Arc::new(Mutex::new(conn));

                        ingest_for_chain(
                            &chain.id,
                            provider,
                            conn.clone(),
                            &repo_client,
                            &config,
                            &mut last_pruned_at_per_chain_id,
                        )
                        .await
                        .unwrap();
                    }

                    interval.tick().await;
                }
            }))
            .await;
    }

    node_task
}

pub fn get_chunked_chains<S: Send + Sync + Clone + 'static>(config: &Config<S>) -> Vec<Vec<Chain>> {
    let chains: Vec<_> = config.chains.clone();
    let chunk_size = max(chains.len() / config.chain_concurrency as usize, 1);

    chains.chunks(chunk_size).map(|c| c.to_vec()).collect()
}

pub async fn ingest_for_chain<'a, S: Send + Sync + Clone>(
    chain_id: &ChainId,
    provider: Arc<impl Provider>,
    conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
    repo_client: &Arc<Mutex<ChaindexingRepoClient>>,
    config @ Config {
        contracts,
        pruning_config,
        ..
    }: &Config<S>,
    last_pruned_at_per_chain_id: &mut HashMap<u64, u64>,
) -> Result<(), IngesterError> {
    let current_block_number = provider::fetch_current_block_number(&provider).await;
    let mut contract_addresses_stream =
        ContractAddressesStream::new(repo_client, *chain_id as i64).with_chunk_size(5);

    while let Some(contract_addresses) = contract_addresses_stream.next().await {
        let contract_addresses =
            filter_uningested_contract_addresses(&contract_addresses, current_block_number);

        let mut conn = conn.lock().await;
        let repo_client = &*repo_client.lock().await;

        ingest_events::run(
            &mut conn,
            repo_client,
            contract_addresses.clone(),
            &provider,
            chain_id,
            current_block_number,
            config,
        )
        .await?;

        maybe_handle_chain_reorg::run(
            &mut conn,
            contract_addresses,
            &provider,
            chain_id,
            current_block_number,
            config,
        )
        .await?;
    }

    maybe_prune(
        pruning_config,
        last_pruned_at_per_chain_id,
        contracts,
        *chain_id as u64,
        current_block_number,
        &*repo_client.lock().await,
    )
    .await;

    Ok(())
}

async fn maybe_prune<S: Send + Sync + Clone>(
    pruning_config: &Option<PruningConfig>,
    last_pruned_at_per_chain_id: &mut HashMap<u64, u64>,
    contracts: &[Contract<S>],
    chain_id: u64,
    current_block_number: u64,
    repo_client: &ChaindexingRepoClient,
) {
    if let Some(pruning_config @ PruningConfig { prune_interval, .. }) = pruning_config {
        let now = Utc::now().timestamp() as u64;
        let last_pruned_at = last_pruned_at_per_chain_id.get(&chain_id).unwrap_or(&now);
        if now - *last_pruned_at >= *prune_interval {
            let min_pruning_block_number =
                pruning_config.get_min_block_number(current_block_number);

            ChaindexingRepo::prune_events(repo_client, min_pruning_block_number, chain_id).await;

            let state_migrations = contracts::get_state_migrations(contracts);
            let state_table_names = states::get_all_table_names(&state_migrations);
            states::prune_state_versions(
                &state_table_names,
                repo_client,
                min_pruning_block_number,
                chain_id,
            )
            .await;
        }
        last_pruned_at_per_chain_id.insert(chain_id, Utc::now().timestamp() as u64);
    }
}

fn filter_uningested_contract_addresses(
    contract_addresses: &[ContractAddress],
    current_block_number: u64,
) -> Vec<ContractAddress> {
    contract_addresses
        .iter()
        .filter(|ca| current_block_number >= ca.next_block_number_to_ingest_from as u64)
        .cloned()
        .collect()
}
