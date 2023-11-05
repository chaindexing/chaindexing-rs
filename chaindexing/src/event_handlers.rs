use std::{sync::Arc, time::Duration};

mod handle_events;
mod maybe_handle_chain_reorg;

use tokio::{sync::Mutex, time::interval};

use crate::{contracts::Contracts, events::Event, ChaindexingRepo, Config, Repo};
use crate::{ChaindexingRepoRawQueryTxnClient, HasRawQueryClient};

#[derive(Clone)]
pub struct EventHandlerContext<'a> {
    pub event: Event,
    raw_query_client: &'a ChaindexingRepoRawQueryTxnClient<'a>,
}

impl<'a> EventHandlerContext<'a> {
    pub fn new(event: Event, client: &'a ChaindexingRepoRawQueryTxnClient<'a>) -> Self {
        Self {
            event,
            raw_query_client: client,
        }
    }

    pub(super) fn get_raw_query_client(&self) -> &'a ChaindexingRepoRawQueryTxnClient<'a> {
        self.raw_query_client
    }
}

#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle_event<'a>(&self, event_context: EventHandlerContext<'a>);
}

// TODO: Use just raw query client through for mutations
pub struct EventHandlers;

impl EventHandlers {
    pub fn start(config: &Config) {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;

            let mut raw_query_client = config.repo.get_raw_query_client().await;

            let conn = Arc::new(Mutex::new(conn));
            let mut interval = interval(Duration::from_millis(config.handler_interval_ms));
            let event_handlers_by_event_abi =
                Contracts::get_all_event_handlers_by_event_abi(&config.contracts);

            loop {
                interval.tick().await;

                handle_events::run(
                    conn.clone(),
                    &event_handlers_by_event_abi,
                    &mut raw_query_client,
                )
                .await;

                let state_migrations = Contracts::get_state_migrations(&config.contracts);
                maybe_handle_chain_reorg::run(
                    conn.clone(),
                    &mut raw_query_client,
                    &state_migrations,
                )
                .await;
            }
        });
    }
}
