#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chaindexing::deferred_futures::DeferredFutures;
    use chaindexing::states::Filters;
    use chaindexing::{ChaindexingRepo, EventContext, HasRawQueryClient};
    use tokio::sync::Mutex;

    use super::*;
    use crate::factory::{bayc_contract, transfer_event_with_contract};
    use crate::test_runner;

    #[tokio::test]
    pub async fn creates_state() {
        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = NftState { token_id: 2 };

        new_state.create(&event_context).await;

        let returned_state =
            NftState::read_one(&Filters::new("token_id", 2), &event_context).await.unwrap();

        assert_eq!(new_state, returned_state);
    }

    #[tokio::test]
    pub async fn updates_state() {
        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = NftState { token_id: 1 };
        new_state.create(&event_context).await;
        new_state.update(&Filters::new("token_id", 4), &event_context).await;

        let initial_state = NftState::read_one(&Filters::new("token_id", 1), &event_context).await;
        assert_eq!(initial_state, None);

        let updated_state = NftState::read_one(&Filters::new("token_id", 4), &event_context).await;
        assert!(updated_state.is_some());
    }

    #[tokio::test]
    pub async fn deletes_state() {
        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = NftState { token_id: 9 };
        new_state.create(&event_context).await;
        new_state.delete(&event_context).await;

        let state = NftState::read_one(&Filters::new("token_id", 9), &event_context).await;
        assert_eq!(state, None);
    }
}

use chaindexing::{
    states::{ContractState, StateMigrations},
    HasRawQueryClient,
};
use serde::{Deserialize, Serialize};

use crate::{factory::bayc_contract, test_runner};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NftState {
    token_id: i32,
}
impl ContractState for NftState {
    fn table_name() -> &'static str {
        "nft_states"
    }
}
struct NftStateMigrations;
impl StateMigrations for NftStateMigrations {
    fn migrations(&self) -> Vec<&'static str> {
        vec![
            "CREATE TABLE IF NOT EXISTS nft_states (
        token_id INTEGER NOT NULL,
    )",
        ]
    }
}

pub async fn setup() {
    let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
    let repo_client = test_runner::new_repo().get_client().await;
    chaindexing::booting::run_user_migrations(&repo_client, &[bayc_contract]).await;
}
