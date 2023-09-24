use chaindexing::{ContractState, Event, EventHandler};

#[derive(Clone, Debug)]
pub enum TestContractState {
    NftState(NftState),
    NftOperatorState(NftOperatorState),
}

impl ContractState for TestContractState {}

#[derive(Clone, Debug)]
pub struct NftState;

impl ContractState for NftState {}

pub struct TransferTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferTestEventHandler {
    type State = TestContractState;
    async fn handle_event(&self, _event: Event) -> Option<Vec<Self::State>> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct NftOperatorState;

impl ContractState for NftOperatorState {}

pub struct ApprovalForAllTestEventHandler;

#[async_trait::async_trait]
impl EventHandler for ApprovalForAllTestEventHandler {
    type State = TestContractState;
    async fn handle_event(&self, _event: Event) -> Option<Vec<Self::State>> {
        None
    }
}
