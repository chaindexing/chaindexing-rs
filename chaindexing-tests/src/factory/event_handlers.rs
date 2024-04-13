use chaindexing::{EventContext, EventHandler};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestEventHandler {
    type SharedState = ();

    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }
    async fn handle_event<'a, 'b>(&self, _event_context: EventContext<'a, 'b, Self::SharedState>) {}
}

pub struct ApprovalForAllTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestEventHandler {
    type SharedState = ();

    fn abi(&self) -> &'static str {
        "event ApprovalForAll(address indexed owner, address indexed operator, bool approved)"
    }
    async fn handle_event<'a, 'b>(&self, _event_context: EventContext<'a, 'b, Self::SharedState>) {}
}
