use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::events::Event;
use crate::{ChaindexingRepoTxnClient, EventParam};

use super::handler_context::HandlerContext;

#[async_trait::async_trait]

pub trait SideEffectHandler: Send + Sync {
    type SharedState: Send + Sync + Clone + Debug;

    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's name is:
    /// PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)
    /// The chain explorer's event section can also be used to easily infer this
    fn abi(&self) -> &'static str;
    async fn handle_event<'a>(
        &self,
        event_context: SideEffectHandlerContext<'a, Self::SharedState>,
    );
}

// SideEffectHandlers are event handlers that help handle side-effects for events.
// This is useful for handling events only ONCE and can rely on a non-deterministic
// shared state. Some use-cases are notifications, bridging etc. Chaindexing ensures
// that the side-effect handlers are called once immutable regardless of resets.
// However, one can dangerously reset including side effects with the new `reset_including_side_effects`
// Config API.
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
