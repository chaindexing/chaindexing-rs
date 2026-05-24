#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::db::database_url;
    use crate::factory::{bayc_contract, BAYC_CONTRACT_ADDRESS, BAYC_CONTRACT_START_BLOCK_NUMBER};
    use crate::{
        find_contract_address_by_contract_name, provider_with_empty_logs,
        provider_with_filter_stubber, provider_with_logs, test_runner,
    };
    use chaindexing::{
        augmenting_std::serde::Deserialize, ingester, ChainId, ChaindexingRepo, Config,
        ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo, Repo,
    };

    #[derive(Debug, Deserialize)]
    struct Count {
        count: i64,
    }

    #[tokio::test]
    pub async fn creates_contract_events() {
        if test_runner::skip_without_test_database() {
            return;
        }

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

            let contract_address = contract_address.to_lowercase();
            assert!(ChaindexingRepo::get_all_events(&mut conn)
                .await
                .iter()
                .all(|event| event.contract_address != contract_address));
            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let mut conn = conn.lock().await;
            let ingested_events = ChaindexingRepo::get_all_events(&mut conn).await;
            let event = ingested_events
                .iter()
                .find(|event| event.contract_address == contract_address)
                .unwrap();
            assert_eq!(event.contract_address, contract_address);
        })
        .await;
    }

    #[tokio::test]
    pub async fn records_block_scans_even_when_no_logs_match() {
        if test_runner::skip_without_test_database() {
            return;
        }

        test_runner::run_test_with_txn(|repo_client, suffix| async move {
            let repo = test_runner::new_repo();
            let pool = repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;

            let bayc_contract = bayc_contract(&format!("BoredApeYachtClub-scans-{suffix}"), "90");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            let provider = Arc::new(provider_with_empty_logs!(BAYC_CONTRACT_ADDRESS));

            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn,
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let repo_client = repo_client.lock().await;
            let count: Count = ChaindexingRepo::load_data(
                &repo_client,
                "SELECT COUNT(*)::BIGINT AS count FROM chaindexing_block_scans",
            )
            .await
            .unwrap();

            assert!(count.count > 0);
        })
        .await;
    }

    #[tokio::test]
    pub async fn starts_from_start_block_number() {
        if test_runner::skip_without_test_database() {
            return;
        }

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
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    pub async fn updates_next_block_number_to_ingest_from_for_a_given_batch() {
        if test_runner::skip_without_test_database() {
            return;
        }

        test_runner::run_test_with_txn(|repo_client, suffix| async move {
            let repo = test_runner::new_repo();
            let pool = repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;

            let bayc_contract = bayc_contract(&format!("BoredApeYachtClub-8-{suffix}"), "03");
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
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let bayc_contract_address = find_contract_address_by_contract_name(
                &repo_client,
                &format!("BoredApeYachtClub-8-{suffix}"),
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

    #[tokio::test]
    pub async fn continues_from_next_block_number_to_ingest_from() {
        if test_runner::skip_without_test_database() {
            return;
        }

        test_runner::run_test_with_txn(|repo_client, suffix| async move {
            let repo = test_runner::new_repo();
            let pool = repo.get_pool(1).await;
            let conn = ChaindexingRepo::get_conn(&pool).await;

            let bayc_contract = bayc_contract(&format!("BoredApeYachtClub-12-{suffix}"), "12");
            let config =
                Config::new(PostgresRepo::new(&database_url())).add_contract(bayc_contract.clone());

            let contract_address = bayc_contract.addresses.first().cloned().unwrap();
            let contract_address = &contract_address.address;
            ChaindexingRepo::create_contract_addresses(&repo_client, &bayc_contract.addresses)
                .await;

            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 50;
            const BLOCKS_PER_BATCH: u64 = 10;
            const EXPECTED_NEXT_BLOCK: u64 =
                BAYC_CONTRACT_START_BLOCK_NUMBER as u64 + BLOCKS_PER_BATCH + 1;
            let first_provider =
                Arc::new(provider_with_logs!(contract_address, CURRENT_BLOCK_NUMBER));
            let conn = Arc::new(Mutex::new(conn));

            let repo_client = Arc::new(Mutex::new(repo_client));
            let config = config.with_blocks_per_batch(BLOCKS_PER_BATCH);
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                first_provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let saw_expected_filter = Arc::new(AtomicBool::new(false));
            let saw_expected_filter_in_stub = saw_expected_filter.clone();
            let second_provider = Arc::new(provider_with_filter_stubber!(
                contract_address,
                CURRENT_BLOCK_NUMBER,
                move |filter: &Filter| {
                    if filter.get_from_block().unwrap().as_u64() == EXPECTED_NEXT_BLOCK {
                        saw_expected_filter_in_stub.store(true, Ordering::SeqCst);
                    }
                }
            ));

            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                second_provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            assert!(saw_expected_filter.load(Ordering::SeqCst));
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_contracts() {
        if test_runner::skip_without_test_database() {
            return;
        }

        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let repo_client = test_runner::new_repo().get_client().await;
            let config: Config<()> = Config::new(PostgresRepo::new(&database_url()));

            #[derive(Clone)]
            struct Provider;

            #[chaindexing::augmenting_std::async_trait]
            impl chaindexing::IngesterProvider for Provider {
                async fn get_block_number(
                    &self,
                ) -> Result<ethers::types::U64, ethers::providers::ProviderError> {
                    Ok(ethers::types::U64::from(0))
                }

                async fn get_logs(
                    &self,
                    _filter: &ethers::types::Filter,
                ) -> Result<Vec<ethers::types::Log>, ethers::providers::ProviderError>
                {
                    panic!("no-contract ingestion must not fetch logs")
                }

                async fn get_block(
                    &self,
                    _block_number: ethers::types::U64,
                ) -> Result<
                    ethers::types::Block<ethers::types::TxHash>,
                    ethers::providers::ProviderError,
                > {
                    panic!("no-contract ingestion must not fetch blocks")
                }
            }

            let provider = Arc::new(Provider);
            let conn = Arc::new(Mutex::new(conn));
            let repo_client = Arc::new(Mutex::new(repo_client));

            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_events_from_contracts() {
        if test_runner::skip_without_test_database() {
            return;
        }

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
            ingester::ingest_for_chain(
                &ChainId::Mainnet,
                provider,
                conn.clone(),
                &repo_client,
                &config,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }
}
