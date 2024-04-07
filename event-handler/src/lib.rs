use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::Mutex;

use events::Event;
use repo_transaction_clients::RepoRawQueryTxnClient;

#[derive(Clone)]
pub struct EventHandlerContext<'a, 'b, SharedState: Sync + Send + Clone> {
    pub event: Event,
    pub raw_query_client: &RepoRawQueryTxnClient<'a>,
    shared_state: &'b Option<Arc<Mutex<SharedState>>>,
}

impl<'a, 'b, SharedState: Sync + Send + Clone> EventHandlerContext<'a, 'b, SharedState> {
    pub fn new(
        event: Event,
        client: &RepoRawQueryTxnClient<'a>,
        shared_state: &'b Option<Arc<Mutex<SharedState>>>,
    ) -> Self {
        Self {
            event,
            raw_query_client: client,
            shared_state,
        }
    }

    pub async fn get_shared_state(&self) -> SharedState {
        let shared_state = self.shared_state.clone().unwrap();
        let shared_state = shared_state.lock().await;
        shared_state.clone()
    }
}

#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    type SharedState: Send + Sync + Clone + Debug;

    async fn handle_event<'a, 'b>(
        &self,
        event_context: EventHandlerContext<'a, 'b, Self::SharedState>,
    );
}
