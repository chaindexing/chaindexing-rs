use derive_more::Display;
use std::sync::Arc;
use uuid::Uuid;

use futures_core::{future::BoxFuture, Stream};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use chain_reorg::{ReorgedBlock, UnsavedReorgedBlock};
use contracts::{ContractAddress, ContractAddressID, UnsavedContractAddress};
use events::{Event, PartialEvent};
use nodes::Node;
use resets::ResetCount;

#[async_trait::async_trait]
pub trait HasRawQueryClient {
    type RawQueryClient: Send + Sync;
    type RawQueryTxnClient<'a>: Send + Sync;

    async fn get_raw_query_client(&self) -> Self::RawQueryClient;
    async fn get_raw_query_txn_client<'a>(
        client: &'a mut Self::RawQueryClient,
    ) -> Self::RawQueryTxnClient<'a>;
}

#[async_trait::async_trait]
pub trait ExecutesWithRawQuery: HasRawQueryClient {
    async fn execute_raw_query(client: &Self::RawQueryClient, query: &str);
    async fn execute_raw_query_in_txn<'a>(client: &Self::RawQueryTxnClient<'a>, query: &str);
    async fn commit_raw_query_txns<'a>(client: Self::RawQueryTxnClient<'a>);

    async fn update_next_block_number_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        contract_address_id: ContractAddressID,
        block_number: i64,
    );

    async fn update_every_next_block_number_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_id: i64,
        block_number: i64,
    );

    async fn update_reorged_blocks_as_handled<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        reorged_block_ids: &[i32],
    );

    async fn prune_events(client: &Self::RawQueryClient, min_block_number: u64, chain_id: u64);
    async fn prune_nodes(client: &Self::RawQueryClient, retain_size: u16);
    async fn prune_reset_counts(client: &Self::RawQueryClient, retain_size: u64);
}

#[async_trait::async_trait]
pub trait LoadsDataWithRawQuery: HasRawQueryClient {
    async fn load_latest_events<'a>(
        client: &Self::RawQueryClient,
        addresses: &Vec<String>,
    ) -> Vec<PartialEvent>;
    async fn load_data_from_raw_query<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Option<Data>;
    async fn load_data_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
        client: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Option<Data>;
    async fn load_data_list_from_raw_query<Data: Send + DeserializeOwned>(
        conn: &Self::RawQueryClient,
        query: &str,
    ) -> Vec<Data>;
    async fn load_data_list_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
        conn: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Vec<Data>;
}
