use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::events::Event;
use crate::outbox::{OutboxReceipt, UnsavedOutboxJob};
use crate::{ChaindexingRepo, ChaindexingRepoTxnClient, EventParam, ExecutesWithRawQuery};

use super::handler_context::HandlerContext;

/// SideEffectHandlers are event handlers that help handle side-effects for events.
/// This is useful for handling events only ONCE and can rely on a non-deterministic
/// shared state. Some use-cases are notifications, bridging etc. Chaindexing ensures
/// that the side-effect handlers are called once immutably regardless of resets.
/// However, one can dangerously reset including side effects with the `reset_including_side_effects`
/// exposed in the Config API.
///
/// For stronger durability, prefer `SideEffectHandlerContext::enqueue_outbox` and dispatch
/// external calls from the durable Postgres outbox.
#[crate::augmenting_std::async_trait]
pub trait SideEffectHandler: Send + Sync {
    type SharedState: Send + Sync + Clone + Debug;

    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's abi is:
    /// `PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)`.
    /// The chain explorer's event section can also be used to infer this.
    fn abi(&self) -> &'static str;
    async fn handle_event<'a>(&self, context: SideEffectHandlerContext<'a, Self::SharedState>);
}

/// Event's context in a side effect handler
#[derive(Clone)]
pub struct SideEffectHandlerContext<'a, SharedState: Sync + Send + Clone> {
    pub event: Event,
    pub(crate) repo_client: &'a ChaindexingRepoTxnClient<'a>,
    shared_state: Option<Arc<Mutex<SharedState>>>,
}

impl<'a, SharedState: Sync + Send + Clone> SideEffectHandlerContext<'a, SharedState> {
    pub fn new(
        event: &Event,
        repo_client: &'a ChaindexingRepoTxnClient<'a>,
        shared_state: &Option<Arc<Mutex<SharedState>>>,
    ) -> Self {
        Self {
            event: event.clone(),
            repo_client,
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

    /// Enqueues a durable, idempotent outbox job for the current event.
    ///
    /// The idempotency key is derived from the handler id and canonical event identity, so
    /// replaying the same event does not enqueue duplicate jobs.
    pub async fn enqueue_outbox<Payload: serde::Serialize>(
        &self,
        handler_id: &str,
        payload: &Payload,
    ) -> OutboxReceipt {
        let job = UnsavedOutboxJob::new(&self.event, handler_id, payload).unwrap();
        let receipt = job.receipt();

        ChaindexingRepo::execute_in_txn(self.repo_client, &job.insert_query()).await;

        receipt
    }
}

impl<'a, SharedState: Sync + Send + Clone> HandlerContext<'a>
    for SideEffectHandlerContext<'a, SharedState>
{
    fn get_event(&self) -> &Event {
        &self.event
    }

    fn get_client(&self) -> &ChaindexingRepoTxnClient<'a> {
        self.repo_client
    }
}
