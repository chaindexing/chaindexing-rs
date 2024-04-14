use std::sync::Arc;

use tokio::sync::Mutex;

use crate::deferred_futures::DeferredFutures;
use crate::events::Event;
use crate::{ChaindexingRepoRawQueryClient, ChaindexingRepoRawQueryTxnClient, EventParam};

use super::handler_context::HandlerContext;

#[async_trait::async_trait]
pub trait PureHandler: Send + Sync {
    /// The human-readable ABI of the event being handled.
    /// For example, Uniswap's PoolCreated event's name is:
    /// PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)
    /// The chain explorer's event section can also be used to easily infer this
    fn abi(&self) -> &'static str;
    async fn handle_event<'a, 'b>(&self, event_context: PureHandlerContext<'a, 'b>);
}

#[derive(Clone)]
pub struct PureHandlerContext<'a, 'b> {
    pub event: Event,
    pub(crate) raw_query_client: &'a ChaindexingRepoRawQueryTxnClient<'a>,
    pub(crate) raw_query_client_for_mcs: Arc<Mutex<ChaindexingRepoRawQueryClient>>,
    pub(crate) deferred_mutations_for_mcs: DeferredFutures<'b>,
}

impl<'a, 'b> PureHandlerContext<'a, 'b> {
    pub fn new(
        event: &Event,
        raw_query_client: &'a ChaindexingRepoRawQueryTxnClient<'a>,
        raw_query_client_for_mcs: &Arc<Mutex<ChaindexingRepoRawQueryClient>>,
        deferred_mutations_for_mcs: &DeferredFutures<'b>,
    ) -> Self {
        Self {
            event: event.clone(),
            raw_query_client,
            raw_query_client_for_mcs: raw_query_client_for_mcs.clone(),
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

    fn get_raw_query_client(&self) -> &ChaindexingRepoRawQueryTxnClient<'a> {
        self.raw_query_client
    }
}
