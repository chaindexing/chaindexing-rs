#[cfg(test)]
mod tests {
    use chaindexing::contract_states::{Filters, Updates};
    use chaindexing::{ChaindexingRepo, EventContext, HasRawQueryClient};

    use super::*;
    use crate::factory::{bayc_contract, transfer_event_with_contract};
    use crate::test_runner;

    #[tokio::test]
    pub async fn creates_state() {
        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
        let mut raw_query_client = test_runner::new_repo().get_raw_query_client().await;
        let raw_query_txn_client =
            ChaindexingRepo::get_raw_query_txn_client(&mut raw_query_client).await;
        let event_context: EventContext<'_, ()> = EventContext::new(
            transfer_event_with_contract(bayc_contract),
            &raw_query_txn_client,
            &None,
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
        let mut raw_query_client = test_runner::new_repo().get_raw_query_client().await;
        let raw_query_txn_client =
            ChaindexingRepo::get_raw_query_txn_client(&mut raw_query_client).await;
        let event_context: EventContext<'_, ()> = EventContext::new(
            transfer_event_with_contract(bayc_contract),
            &raw_query_txn_client,
            &None,
        );

        let new_state = NftState { token_id: 1 };
        new_state.create(&event_context).await;
        new_state.update(&Updates::new("token_id", 4), &event_context).await;

        let initial_state = NftState::read_one(&Filters::new("token_id", 1), &event_context).await;
        assert_eq!(initial_state, None);

        let updated_state = NftState::read_one(&Filters::new("token_id", 4), &event_context).await;
        assert!(updated_state.is_some());
    }

    #[tokio::test]
    pub async fn deletes_state() {
        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);
        let mut raw_query_client = test_runner::new_repo().get_raw_query_client().await;
        let raw_query_txn_client =
            ChaindexingRepo::get_raw_query_txn_client(&mut raw_query_client).await;
        let event_context: EventContext<'_, ()> = EventContext::new(
            transfer_event_with_contract(bayc_contract),
            &raw_query_txn_client,
            &None,
        );

        let new_state = NftState { token_id: 9 };
        new_state.create(&event_context).await;
        new_state.delete(&event_context).await;

        let state = NftState::read_one(&Filters::new("token_id", 9), &event_context).await;
        assert_eq!(state, None);
    }
}

use chaindexing::{
    contract_states::{ContractState, ContractStateMigrations},
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
impl ContractStateMigrations for NftStateMigrations {
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
    let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
    chaindexing::run_migrations_for_contract_states(&raw_query_client, &[bayc_contract]).await;
}
