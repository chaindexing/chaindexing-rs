use chaindexing::{EventContext, EventHandler, NoSharedState};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestEventHandler {
    type SharedState = NoSharedState;

    async fn handle_event<'a>(&self, _event_context: EventContext<'a, Self::SharedState>) {}
}

pub struct ApprovalForAllTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestEventHandler {
    type SharedState = NoSharedState;

    async fn handle_event<'a>(&self, _event_context: EventContext<'a, Self::SharedState>) {}
}
