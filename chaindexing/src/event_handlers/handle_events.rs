use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::{events::Event, ChaindexingRepo};
use crate::{
    ChaindexingRepoConn, ChaindexingRepoRawQueryClient, ContractAddress, ExecutesWithRawQuery,
    HasRawQueryClient, Streamable,
};

use super::{EventHandler, EventHandlerContext};

pub async fn run<'a, S: Send + Sync + Clone + Debug>(
    conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
    event_handlers_by_event_abi: &HashMap<&str, Arc<dyn EventHandler<SharedState = S>>>,
    raw_query_client: &mut ChaindexingRepoRawQueryClient,
    shared_state: Option<Arc<Mutex<S>>>,
) {
    let mut contract_addresses_stream =
        ChaindexingRepo::get_contract_addresses_stream(conn.clone());

    while let Some(contract_addresses) = contract_addresses_stream.next().await {
        for contract_address in contract_addresses {
            handle_events_for_contract_address(
                conn.clone(),
                &contract_address,
                event_handlers_by_event_abi,
                raw_query_client,
                shared_state.clone(),
            )
            .await
        }
    }
}

async fn handle_events_for_contract_address<'a, S: Send + Sync + Clone + Debug>(
    conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
    contract_address: &ContractAddress,
    event_handlers_by_event_abi: &HashMap<&str, Arc<dyn EventHandler<SharedState = S>>>,
    raw_query_client: &mut ChaindexingRepoRawQueryClient,
    shared_state: Option<Arc<Mutex<S>>>,
) {
    let mut events_stream = ChaindexingRepo::get_events_stream(
        conn.clone(),
        contract_address.next_block_number_to_handle_from,
        contract_address.chain_id,
        contract_address.address.clone(),
    );

    while let Some(mut events) = events_stream.next().await {
        events.sort_by_key(|e| (e.block_number, e.log_index));

        let raw_query_txn_client =
            ChaindexingRepo::get_raw_query_txn_client(raw_query_client).await;

        for event in &events {
            let event_handler = event_handlers_by_event_abi.get(event.abi.as_str()).unwrap();
            let event_handler_context = EventHandlerContext::new(
                event.clone(),
                &raw_query_txn_client,
                shared_state.clone(),
            );

            event_handler.handle_event(event_handler_context).await;
        }

        if let Some(Event { block_number, .. }) = events.last() {
            let next_block_number_to_handle_from = block_number + 1;
            ChaindexingRepo::update_next_block_number_to_handle_from_in_txn(
                &raw_query_txn_client,
                contract_address.id(),
                next_block_number_to_handle_from,
            )
            .await;
        }

        ChaindexingRepo::commit_raw_query_txns(raw_query_txn_client).await;
    }
}
