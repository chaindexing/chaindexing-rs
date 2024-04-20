use chaindexing::{EventContext, EventHandler};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestHandler;

#[chaindexing::augmenting_std::async_trait::async_trait]
impl EventHandler for TransferTestHandler {
    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }
    async fn handle_event<'a, 'b>(&self, _context: EventContext<'a, 'b>) {}
}

pub struct ApprovalForAllTestHandler;

#[chaindexing::augmenting_std::async_trait::async_trait]
impl EventHandler for ApprovalForAllTestHandler {
    fn abi(&self) -> &'static str {
        "event ApprovalForAll(address indexed owner, address indexed operator, bool approved)"
    }
    async fn handle_event<'a, 'b>(&self, _context: EventContext<'a, 'b>) {}
}
