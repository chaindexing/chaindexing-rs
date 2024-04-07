mod error;
mod filters;
mod ingest_events;
mod maybe_handle_chain_reorg;
mod provider;

pub use error::EventsIngesterError;
pub use provider::{Provider, ProviderError};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use tokio::time::interval;
use tokio::{sync::Mutex, task};

use crate::chains::{Chain, ChainId};
use crate::contracts::{Contract, Contracts};
use crate::pruning::PruningConfig;
use crate::Config;
use crate::MinConfirmationCount;
use crate::{ChaindexingRepo, ChaindexingRepoConn, ChaindexingRepoRawQueryClient};
use crate::{ContractAddress, ContractStates};
use crate::{ExecutesWithRawQuery, HasRawQueryClient, Repo, Streamable};

pub fn start<S: Sync + Send + Clone + 'static>(config: &Config<S>) -> task::JoinHandle<()> {
    let config = config.clone();
    tokio::spawn(async move {
        let pool = config.repo.get_pool(1).await;
        let conn = ChaindexingRepo::get_conn(&pool).await;
        let conn = Arc::new(Mutex::new(conn));

        let raw_query_client = config.repo.get_raw_query_client().await;

        let mut interval = interval(Duration::from_millis(config.ingestion_rate_ms));

        let mut last_pruned_at_per_chain_id = HashMap::new();

        loop {
            for chain @ Chain { json_rpc_url, .. } in config.chains.iter() {
                let provider = provider::get(json_rpc_url);

                ingest(
                    conn.clone(),
                    &raw_query_client,
                    &config.contracts,
                    config.blocks_per_batch,
                    provider,
                    &chain.id,
                    &config.min_confirmation_count,
                    &config.pruning_config,
                    &mut last_pruned_at_per_chain_id,
                )
                .await
                .unwrap();
            }

            interval.tick().await;
        }
    })
}

pub async fn ingest<'a, S: Send + Sync + Clone>(
    conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
    raw_query_client: &ChaindexingRepoRawQueryClient,
    contracts: &Vec<Contract<S>>,
    blocks_per_batch: u64,
    provider: Arc<impl Provider + 'static>,
    chain_id: &ChainId,
    min_confirmation_count: &MinConfirmationCount,
    pruning_config: &Option<PruningConfig>,
    last_pruned_at_per_chain_id: &mut HashMap<ChainId, u64>,
) -> Result<(), EventsIngesterError> {
    let current_block_number = provider::fetch_current_block_number(&provider).await;
    let mut contract_addresses_stream =
        ChaindexingRepo::get_contract_addresses_stream_by_chain(conn.clone(), *chain_id as i64);

    while let Some(contract_addresses) = contract_addresses_stream.next().await {
        let contract_addresses =
            filter_uningested_contract_addresses(&contract_addresses, current_block_number);

        let mut conn = conn.lock().await;

        ingest_events::run(
            &mut conn,
            raw_query_client,
            &contract_addresses,
            contracts,
            &provider,
            current_block_number,
            blocks_per_batch,
        )
        .await?;

        maybe_handle_chain_reorg::run(
            &mut conn,
            &contract_addresses,
            contracts,
            &provider,
            chain_id,
            current_block_number,
            blocks_per_batch,
            min_confirmation_count,
        )
        .await?;

        if let Some(pruning_config @ PruningConfig { prune_interval, .. }) = pruning_config {
            let now = Utc::now().timestamp() as u64;
            let last_pruned_at = last_pruned_at_per_chain_id.get(chain_id).unwrap_or(&now);
            let chain_id_u64 = *chain_id as u64;
            if now - *last_pruned_at >= *prune_interval {
                let min_pruning_block_number =
                    pruning_config.get_min_block_number(current_block_number);
                ChaindexingRepo::prune_events(
                    raw_query_client,
                    min_pruning_block_number,
                    chain_id_u64,
                )
                .await;

                let state_migrations = Contracts::get_state_migrations(&contracts);
                let state_table_names = ContractStates::get_all_table_names(&state_migrations);
                ContractStates::prune_state_versions(
                    &state_table_names,
                    &raw_query_client,
                    min_pruning_block_number,
                    chain_id_u64,
                )
                .await;
            }
            last_pruned_at_per_chain_id.insert(*chain_id, Utc::now().timestamp() as u64);
        }
    }

    Ok(())
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
