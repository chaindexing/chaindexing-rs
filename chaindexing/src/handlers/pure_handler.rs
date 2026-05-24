use std::sync::Arc;

use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::events::Event;
use crate::{ChaindexingRepoClient, ChaindexingRepoTxnClient, EventParam};

use super::handler_context::HandlerContext;

/// Pure handlers do not contain any side effects. They are simple reducers
/// that derive or index states deterministically.
#[crate::augmenting_std::async_trait]
pub trait PureHandler: Send + Sync {
    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's abi is:
    /// `PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)`.
    /// The chain explorer's event section can also be used to infer this.
    fn abi(&self) -> &'static str;
    async fn handle_event<'a, 'b>(&self, context: PureHandlerContext<'a, 'b>);
}

/// Event's context in a pure event handler
#[derive(Clone)]
pub struct PureHandlerContext<'a, 'b> {
    pub event: Event,
    pub(crate) repo_client: &'a ChaindexingRepoTxnClient<'a>,
    pub(crate) repo_client_for_mcs: Arc<Mutex<ChaindexingRepoClient>>,
    pub(crate) deferred_mutations_for_mcs: DeferredFutures<'b>,
    is_at_block_tail: bool,
}

impl<'a, 'b> PureHandlerContext<'a, 'b> {
    pub fn new(
        event: &Event,
        repo_client: &'a ChaindexingRepoTxnClient<'a>,
        repo_client_for_mcs: &Arc<Mutex<ChaindexingRepoClient>>,
        deferred_mutations_for_mcs: &DeferredFutures<'b>,
    ) -> Self {
        Self {
            event: event.clone(),
            repo_client,
            repo_client_for_mcs: repo_client_for_mcs.clone(),
            deferred_mutations_for_mcs: deferred_mutations_for_mcs.clone(),
            is_at_block_tail: false,
        }
    }

    pub fn get_event_params(&self) -> EventParam {
        self.event.get_params()
    }

    /// Returns true when this event is at the latest block already ingested for
    /// its contract address.
    ///
    /// This is an application heuristic for distinguishing catch-up/backfill
    /// handling from the indexed tail. It is not a global chain-head or finality
    /// guarantee.
    pub fn is_at_block_tail(&self) -> bool {
        self.is_at_block_tail
    }

    pub(crate) fn with_is_at_block_tail(mut self, is_at_block_tail: bool) -> Self {
        self.is_at_block_tail = is_at_block_tail;
        self
    }
}

impl<'a, 'b> HandlerContext<'a> for PureHandlerContext<'a, 'b> {
    fn get_event(&self) -> &Event {
        &self.event
    }

    fn get_client(&self) -> &ChaindexingRepoTxnClient<'a> {
        self.repo_client
    }
}
