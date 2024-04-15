#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::db::database_url;
    use crate::factory::{bayc_contract, empty_provider, BAYC_CONTRACT_START_BLOCK_NUMBER};
    use crate::{
        find_contract_address_by_contract_name, provider_with_empty_logs,
        provider_with_filter_stubber, provider_with_logs, test_runner,
    };
    use chaindexing::{
        ingester, ChainId, ChaindexingRepo, Config, ExecutesWithRawQuery, HasRawQueryClient,
        PostgresRepo, Repo,
    };

    #[tokio::test]
    pub async fn creates_contract_events() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let bayc_contract = bayc_contract("BoredApeYachtClub-9", "01");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let contract_address = bayc_contract.addresses.first().cloned().unwrap();
            let contract_address = &contract_address.address;
            let provider = Arc::new(provider_with_logs!(&contract_address, CURRENT_BLOCK_NUMBER));

            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));
            ingester::ingest(
                conn.clone(),
                &repo_client,
                provider,
                &ChainId::Mainnet,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let mut conn = conn.lock().await;
            let ingested_events = ChaindexingRepo::get_all_events(&mut conn).await;
            let first_event = ingested_events.first().unwrap();
            assert_eq!(
                first_event.contract_address,
                contract_address.to_lowercase()
            );
        })
        .await;
    }

    #[tokio::test]
    pub async fn starts_from_start_block_number() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let bayc_contract = bayc_contract("BoredApeYachtClub-10", "02");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;
            let provider = Arc::new(provider_with_filter_stubber!(
                BAYC_CONTRACT_ADDRESS,
                |filter: &Filter| {
                    assert_eq!(
                        filter.get_from_block().unwrap().as_u32(),
                        BAYC_CONTRACT_START_BLOCK_NUMBER
                    );
                }
            ));

            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));
            ingester::ingest(
                conn.clone(),
                &repo_client,
                provider,
                &ChainId::Mainnet,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }

    // Remove ignore after refactoring EventingIngester to no use diesel
    // Currently, it fails because we stream contract addresses
    // outside the diesel transaction session
    #[ignore]
    #[tokio::test]
    pub async fn updates_next_block_number_to_ingest_from_for_a_given_batch() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let bayc_contract = bayc_contract("BoredApeYachtClub-8", "03");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let contract_address = bayc_contract.addresses.first().cloned().unwrap();
            let contract_address = &contract_address.address;
            let provider = Arc::new(provider_with_logs!(contract_address, CURRENT_BLOCK_NUMBER));

            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            let conn = Arc::new(Mutex::new(conn));
            let blocks_per_batch = 10;

            let repo_client = Arc::new(Mutex::new(repo_client));
            let config = config.with_blocks_per_batch(blocks_per_batch);
            ingester::ingest(
                conn.clone(),
                &repo_client,
                provider,
                &ChainId::Mainnet,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let bayc_contract_address = find_contract_address_by_contract_name(
                &repo_client,
                "BoredApeYachtClub-8",
                &ChainId::Mainnet,
            )
            .await
            .unwrap();
            let next_block_number_to_ingest_from =
                bayc_contract_address.next_block_number_to_ingest_from as u64;
            assert_eq!(
                next_block_number_to_ingest_from,
                BAYC_CONTRACT_START_BLOCK_NUMBER as u64 + blocks_per_batch + 1
            );
        })
        .await;
    }

    // TODO:
    #[tokio::test]
    pub async fn continues_from_next_block_number_to_ingest_from() {}

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let config: Config<()> = Config::new(PostgresRepo::new(&database_url()));

            let provider = Arc::new(empty_provider());
            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));

            ingester::ingest(
                conn.clone(),
                &repo_client,
                provider,
                &ChainId::Mainnet,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
            let mut conn = conn.lock().await;
            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_events_from_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let bayc_contract = bayc_contract("BoredApeYachtClub-11", "04");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            let provider = Arc::new(provider_with_empty_logs!(BAYC_CONTRACT_ADDRESS));

            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));
            ingester::ingest(
                conn.clone(),
                &repo_client,
                provider,
                &ChainId::Mainnet,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }
}
