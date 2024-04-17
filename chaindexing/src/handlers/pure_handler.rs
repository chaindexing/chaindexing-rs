use std::sync::Arc;

use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::events::Event;
use crate::{ChaindexingRepoClient, ChaindexingRepoTxnClient, EventParam};

use super::handler_context::HandlerContext;

#[async_trait::async_trait]
pub trait PureHandler: Send + Sync {
    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's name is:
    /// PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)
    /// The chain explorer's event section can also be used to easily infer this
    fn abi(&self) -> &'static str;
    async fn handle_event<'a, 'b>(&self, context: PureHandlerContext<'a, 'b>);
}

#[derive(Clone)]
pub struct PureHandlerContext<'a, 'b> {
    pub event: Event,
    pub(crate) repo_client: &'a ChaindexingRepoTxnClient<'a>,
    pub(crate) repo_client_for_mcs: Arc<Mutex<ChaindexingRepoClient>>,
    pub(crate) deferred_mutations_for_mcs: DeferredFutures<'b>,
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
        }
    }

    pub fn get_event_params(&self) -> EventParam {
        self.event.get_params()
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
