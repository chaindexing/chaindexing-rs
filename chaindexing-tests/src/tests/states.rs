#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chaindexing::deferred_futures::DeferredFutures;
    use chaindexing::states::{Filters, Updates};
    use chaindexing::{ChaindexingRepo, EventContext, HasRawQueryClient};
    use tokio::sync::Mutex;

    use super::*;
    use crate::factory::{bayc_contract, transfer_event_with_contract};
    use crate::test_runner;

    #[tokio::test]
    pub async fn creates_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-1", "09").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = Nft { token_id: 2 };

        new_state.create(&event_context).await;

        let returned_state =
            Nft::read_one(&Filters::new("token_id", 2), &event_context).await.unwrap();

        assert_eq!(new_state, returned_state);
    }

    #[tokio::test]
    pub async fn updates_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-2", "07").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = Nft { token_id: 1 };
        new_state.create(&event_context).await;
        new_state.update(&Updates::new("token_id", 4), &event_context).await;

        let initial_state = Nft::read_one(&Filters::new("token_id", 1), &event_context).await;
        assert_eq!(initial_state, None);

        let updated_state = Nft::read_one(&Filters::new("token_id", 4), &event_context).await;
        assert!(updated_state.is_some());
    }

    #[tokio::test]
    pub async fn deletes_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-3", "05").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let new_state = Nft { token_id: 9 };
        new_state.create(&event_context).await;
        new_state.delete(&event_context).await;

        let state = Nft::read_one(&Filters::new("token_id", 9), &event_context).await;
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
struct Nft {
    token_id: i32,
}
impl ContractState for Nft {
    fn table_name() -> &'static str {
        "nfts"
    }
}
struct NftMigrations;
impl StateMigrations for NftMigrations {
    fn migrations(&self) -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS nfts (
            token_id INTEGER NOT NULL)"]
    }
}

pub async fn setup() {
    let bayc_contract =
        bayc_contract("BoredApeYachtClub", "06").add_state_migrations(NftMigrations);
    let repo_client = test_runner::new_repo().get_client().await;
    chaindexing::booting::run_user_migrations(&repo_client, &[bayc_contract]).await;
}
