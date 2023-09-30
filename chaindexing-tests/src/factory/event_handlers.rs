use chaindexing::{ChaindexingRepoRawQueryTxnClient, ContractStateError, Event, EventHandler};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestEventHandler {
    async fn handle_event<'a>(
        &self,
        _event: Event,
        _client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<(), ContractStateError> {
        Ok(())
    }
}

pub struct ApprovalForAllTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestEventHandler {
    async fn handle_event<'a>(
        &self,
        _event: Event,
        _client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<(), ContractStateError> {
        Ok(())
    }
}
