use chaindexing::{ChaindexingRepoRawQueryClient, ContractStateError, Event, EventHandler};

#[derive(Clone, Debug)]
pub struct NftState;

pub struct TransferTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestEventHandler {
    async fn handle_event(
        &self,
        _event: Event,
        _client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        Ok(())
    }
}

pub struct ApprovalForAllTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestEventHandler {
    async fn handle_event(
        &self,
        _event: Event,
        _client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        Ok(())
    }
}
