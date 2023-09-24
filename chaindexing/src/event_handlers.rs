use std::{collections::HashMap, sync::Arc, time::Duration};

use futures_util::future::join_all;
use futures_util::{stream, StreamExt};
use tokio::{sync::Mutex, time::interval};

use crate::{contracts::Contracts, events::Event, ChaindexingRepo, Config, Repo};
use crate::{ChaindexingRepoConn, ContractAddress, ContractState, Streamable};

#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    type State: ContractState;
    async fn handle_event(&self, event: Event) -> Option<Vec<Self::State>>;
}

pub trait AllEventHandlers {}

pub struct EventHandlers;

impl EventHandlers {
    pub fn start<State: ContractState>(config: &Config<State>) {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;
            let conn = Arc::new(Mutex::new(conn));
            let mut interval = interval(Duration::from_millis(config.handler_interval_ms));
            let event_handlers_by_event_abi =
                Contracts::get_all_event_handlers_by_event_abi(&config.contracts);

            loop {
                interval.tick().await;

                Self::handle_events(conn.clone(), &event_handlers_by_event_abi).await;
            }
        });
    }

    pub async fn handle_events<'a, State: ContractState>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        event_handlers_by_event_abi: &HashMap<&str, Arc<dyn EventHandler<State = State>>>,
    ) {
        let mut contract_addresses_stream =
            ChaindexingRepo::get_contract_addresses_stream(conn.clone());

        while let Some(contract_addresses) = contract_addresses_stream.next().await {
            stream::iter(contract_addresses)
                .for_each(|contract_address| {
                    let conn = conn.clone();

                    async move {
                        Self::handle_event_for_contract_address(
                            conn,
                            &contract_address,
                            event_handlers_by_event_abi,
                        )
                        .await
                    }
                })
                .await;
        }
    }

    pub async fn handle_event_for_contract_address<'a, State: ContractState>(
        conn: Arc<Mutex<ChaindexingRepoConn<'a>>>,
        contract_address: &ContractAddress,
        event_handlers_by_event_abi: &HashMap<&str, Arc<dyn EventHandler<State = State>>>,
    ) {
        let mut events_stream = ChaindexingRepo::get_events_stream(
            conn.clone(),
            contract_address.next_block_number_to_handle_from,
        );

        while let Some(events) = events_stream.next().await {
            // TODO: Move this filter to the stream query level
            let mut events: Vec<Event> = events
                .into_iter()
                .filter(|event| event.match_contract_address(&contract_address.address))
                .collect();
            events.sort_by_key(|e| (e.block_number, e.log_index));

            join_all(events.iter().map(|event| {
                let event_handler = event_handlers_by_event_abi.get(event.abi.as_str()).unwrap();

                event_handler.handle_event(event.clone())
            }))
            .await;

            let mut conn = conn.lock().await;

            if let Some(Event { block_number, .. }) = events.last() {
                let next_block_number_to_handle_from = block_number + 1;
                ChaindexingRepo::update_next_block_number_to_handle_from(
                    &mut conn,
                    contract_address.id(),
                    next_block_number_to_handle_from,
                )
                .await;
            }
        }
    }
}
