#[cfg(test)]
mod create {
    use chaindexing::{Chaindexing, ChaindexingRepo, EventContext, HasRawQueryClient};
    use chaindexing::{ContractState, ContractStateMigrations};
    use serde::{Deserialize, Serialize};

    use crate::{factory::*, test_runner};

    #[tokio::test]
    pub async fn creates_state() {
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

        let bayc_contract = bayc_contract().add_state_migrations(NftStateMigrations);

        let mut raw_query_client = test_runner::new_repo().get_raw_query_client().await;

        Chaindexing::run_migrations_for_contract_states(
            &raw_query_client,
            &vec![bayc_contract.clone()],
        )
        .await;

        let raw_query_txn_client =
            ChaindexingRepo::get_raw_query_txn_client(&mut raw_query_client).await;

        let event_context = EventContext::new(
            transfer_event_with_contract(bayc_contract),
            &raw_query_txn_client,
        );

        let new_state = NftState { token_id: 2 };

        new_state.create(&event_context).await;

        let returned_state = NftState::read_one(
            [("token_id".to_owned(), "2".to_owned())].into(),
            &event_context,
        )
        .await
        .unwrap();

        assert_eq!(new_state, returned_state);
    }
}

#[cfg(test)]
mod update {
    #[tokio::test]
    pub async fn updates_state() {}
}

#[cfg(test)]
mod delete {
    #[tokio::test]
    pub async fn deletes_state() {}
}

#[cfg(test)]
mod read_many {
    #[tokio::test]
    pub async fn returns_states_matching_filter() {}

    #[tokio::test]
    pub async fn returns_empty_if_no_state_matches_the_filter() {}

    #[tokio::test]
    pub async fn does_not_return_future_state() {}
}
