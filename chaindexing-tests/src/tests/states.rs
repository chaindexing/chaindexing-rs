#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chaindexing::deferred_futures::DeferredFutures;
    use chaindexing::states::{Filters, Updates};
    use chaindexing::{ChaindexingRepo, EventContext, HasRawQueryClient};
    use tokio::sync::Mutex;

    use super::*;
    use crate::factory::{bayc_contract, unique_transfer_event_with_contract};
    use crate::test_runner;

    /// Generate a unique token ID for this test to avoid conflicts
    fn generate_unique_token_id() -> i32 {
        use std::sync::atomic::{AtomicI32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static TOKEN_COUNTER: AtomicI32 = AtomicI32::new(0);

        let counter = TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst);
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i32;
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<i32>()
            .unwrap_or(1);

        // Generate a unique token ID that's very unlikely to collide
        // Using a large range to minimize collision probability
        100_000 + (timestamp % 10_000) + (counter % 1_000) + (thread_id % 100)
    }

    #[tokio::test]
    pub async fn creates_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-1", "09").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        let event_context: EventContext<'_, '_> = EventContext::new(
            &unique_transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let token_id = generate_unique_token_id();
        let new_state = Nft { token_id };

        new_state.create(&event_context).await;

        let returned_state = Nft::read_one(&Filters::new("token_id", token_id), &event_context)
            .await
            .unwrap();

        assert_eq!(new_state, returned_state);
    }

    #[tokio::test]
    pub async fn updates_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-2", "07").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;

        // Create first event context for the create operation
        let create_event_context: EventContext<'_, '_> = EventContext::new(
            &unique_transfer_event_with_contract(bayc_contract.clone()),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let initial_token_id = generate_unique_token_id();
        let updated_token_id = initial_token_id + 1; // Ensure it's different but related

        let new_state = Nft {
            token_id: initial_token_id,
        };
        new_state.create(&create_event_context).await;

        // Create second event context for the update operation with different blockchain metadata
        let update_event_context: EventContext<'_, '_> = EventContext::new(
            &unique_transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        new_state
            .update(
                &Updates::new("token_id", updated_token_id),
                &update_event_context,
            )
            .await;

        let initial_state = Nft::read_one(
            &Filters::new("token_id", initial_token_id),
            &create_event_context,
        )
        .await;
        assert_eq!(initial_state, None);

        let updated_state = Nft::read_one(
            &Filters::new("token_id", updated_token_id),
            &create_event_context,
        )
        .await;
        assert!(updated_state.is_some());
    }

    #[tokio::test]
    pub async fn deletes_state() {
        let bayc_contract =
            bayc_contract("BoredApeYachtClub-3", "05").add_state_migrations(NftMigrations);
        let mut repo_client = test_runner::new_repo().get_client().await;
        let repo_txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;

        // Create first event context for the create operation
        let create_event_context: EventContext<'_, '_> = EventContext::new(
            &unique_transfer_event_with_contract(bayc_contract.clone()),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        let token_id = generate_unique_token_id();
        let new_state = Nft { token_id };
        new_state.create(&create_event_context).await;

        // Create second event context for the delete operation with different blockchain metadata
        let delete_event_context: EventContext<'_, '_> = EventContext::new(
            &unique_transfer_event_with_contract(bayc_contract),
            &repo_txn_client,
            &Arc::new(Mutex::new(test_runner::new_repo().get_client().await)),
            &DeferredFutures::new(),
        );

        new_state.delete(&delete_event_context).await;

        let state = Nft::read_one(&Filters::new("token_id", token_id), &create_event_context).await;
        assert_eq!(state, None);
    }
}

use chaindexing::augmenting_std::serde::{Deserialize, Serialize};
use chaindexing::{
    states::{ContractState, StateMigrations},
    HasRawQueryClient,
};

use crate::{factory::bayc_contract, test_runner};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "chaindexing::augmenting_std::serde")]
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
