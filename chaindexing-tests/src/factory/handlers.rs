use chaindexing::{EventContext, EventHandler};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestHandler {
    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }
    async fn handle_event<'a, 'b>(&self, _event_context: EventContext<'a, 'b>) {}
}

pub struct ApprovalForAllTestHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestHandler {
    fn abi(&self) -> &'static str {
        "event ApprovalForAll(address indexed owner, address indexed operator, bool approved)"
    }
    async fn handle_event<'a, 'b>(&self, _event_context: EventContext<'a, 'b>) {}
}