pub mod db;
pub mod factory;
pub mod test_runner;
pub mod tests;

use std::sync::Arc;

use chaindexing::{
    streams::ContractAddressesStream, ChainId, ChaindexingRepoClient, ContractAddress,
};
use futures_util::StreamExt;
use tokio::sync::Mutex;

pub async fn find_contract_address_by_contract_name(
    repo_client: &Arc<Mutex<ChaindexingRepoClient>>,
    contract_name: &str,
    chain_id: &ChainId,
) -> Option<ContractAddress> {
    let mut contract_addresses_stream = ContractAddressesStream::new(repo_client, *chain_id as i64);
    contract_addresses_stream
        .next()
        .await
        .iter()
        .flatten()
        .find(|ca| ca.contract_name == contract_name)
        .cloned()
}
