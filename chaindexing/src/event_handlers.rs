use std::fmt::Debug;
use std::{sync::Arc, time::Duration};

mod handle_events;
mod maybe_handle_chain_reorg;

use tokio::{sync::Mutex, task, time::interval};

use crate::{contracts::Contracts, events::Event, ChaindexingRepo, Config, Repo};
use crate::{ChaindexingRepoRawQueryTxnClient, ContractStates, EventParam, HasRawQueryClient};

#[derive(Clone)]
pub struct EventHandlerContext<'a, SharedState: Sync + Send + Clone> {
    pub event: Event,
    pub(super) raw_query_client: &'a ChaindexingRepoRawQueryTxnClient<'a>,
    shared_state: Option<Arc<Mutex<SharedState>>>,
}

impl<'a, SharedState: Sync + Send + Clone> EventHandlerContext<'a, SharedState> {
    pub fn new(
        event: Event,
        client: &'a ChaindexingRepoRawQueryTxnClient<'a>,
        shared_state: &Option<Arc<Mutex<SharedState>>>,
    ) -> Self {
        Self {
            event,
            raw_query_client: client,
            shared_state: shared_state.clone(),
        }
    }

    pub async fn get_shared_state(&self) -> SharedState {
        let shared_state = self.shared_state.clone().unwrap();
        let shared_state = shared_state.lock().await;
        shared_state.clone()
    }

    pub fn get_event_params(&self) -> EventParam {
        self.event.get_params()
    }
}

#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    type SharedState: Send + Sync + Clone + Debug;

    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's name is:
    /// PoolCreated(index_topic_1 address token0, index_topic_2 address token1, index_topic_3 uint24 fee, int24 tickSpacing, address pool)
    /// The chain explorer's event section of the indexed contract
    /// can also be used to infer this
    fn abi(&self) -> &'static str;
    async fn handle_event<'a>(&self, event_context: EventHandlerContext<'a, Self::SharedState>);
}

// TODO: Use just raw query client through for mutations
pub struct EventHandlers;

impl EventHandlers {
    pub fn start<S: Send + Sync + Clone + Debug + 'static>(
        config: &Config<S>,
    ) -> task::JoinHandle<()> {
        let config = config.clone();
        tokio::spawn(async move {
            let pool = config.repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;

            let mut raw_query_client = config.repo.get_raw_query_client().await;

            let conn = Arc::new(Mutex::new(conn));
            let mut interval = interval(Duration::from_millis(config.handler_rate_ms));
            let event_handlers_by_event_abi =
                Contracts::get_all_event_handlers_by_event_abi(&config.contracts);

            loop {
                handle_events::run(
                    conn.clone(),
                    &event_handlers_by_event_abi,
                    &mut raw_query_client,
                    &config.shared_state,
                )
                .await;

                let state_migrations = Contracts::get_state_migrations(&config.contracts);
                let state_table_names = ContractStates::get_all_table_names(&state_migrations);

                maybe_handle_chain_reorg::run(
                    conn.clone(),
                    &mut raw_query_client,
                    &state_table_names,
                )
                .await;

                interval.tick().await;
            }
        })
    }
}
