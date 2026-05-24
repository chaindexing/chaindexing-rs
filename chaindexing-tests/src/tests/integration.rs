#[cfg(test)]
mod postgres_integration {
    use std::env;
    use std::sync::Arc;

    use chaindexing::{
        augmenting_std::serde::Deserialize, booting, dispatch_pending_outbox_jobs, ChainId,
        ChaindexingRepo, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery,
        OutboxDispatchConfig, OutboxDispatcher, OutboxJob, Repo, UnsavedContractAddress,
    };
    use dotenvy::dotenv;
    use futures_util::StreamExt;
    use tokio::sync::Mutex;

    use crate::db;
    use crate::factory;
    use crate::test_runner;

    async fn setup_postgres() -> Option<(String, chaindexing::ChaindexingRepoClient)> {
        dotenv().ok();
        if test_runner::skip_without_test_database() {
            return None;
        }

        let database_url = env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set");

        db::setup();
        let repo = ChaindexingRepo::new(&database_url);
        let repo_client = repo.get_client().await;
        booting::setup_root(&repo_client).await;
        booting::run_internal_migrations(&repo_client).await;

        Some((database_url, repo_client))
    }

    #[derive(Debug, Deserialize)]
    struct TableName {
        table_name: String,
    }

    #[tokio::test]
    async fn fresh_install_creates_internal_tables() {
        let Some((_database_url, repo_client)) = setup_postgres().await else {
            return;
        };

        let tables: Vec<TableName> = ChaindexingRepo::load_data_list(
            &repo_client,
            "SELECT table_name
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name IN (
                   'chaindexing_events',
                   'chaindexing_checkpoints',
                   'chaindexing_blocks',
                   'chaindexing_transactions',
                   'chaindexing_call_traces',
                   'chaindexing_block_scans',
                   'chaindexing_reorgs',
                   'chaindexing_outbox'
               )",
        )
        .await;
        let table_names: Vec<_> = tables.into_iter().map(|table| table.table_name).collect();

        assert!(table_names.contains(&"chaindexing_events".to_string()));
        assert!(table_names.contains(&"chaindexing_checkpoints".to_string()));
        assert!(table_names.contains(&"chaindexing_blocks".to_string()));
        assert!(table_names.contains(&"chaindexing_transactions".to_string()));
        assert!(table_names.contains(&"chaindexing_call_traces".to_string()));
        assert!(table_names.contains(&"chaindexing_block_scans".to_string()));
        assert!(table_names.contains(&"chaindexing_reorgs".to_string()));
        assert!(table_names.contains(&"chaindexing_outbox".to_string()));
    }

    #[derive(Debug, Deserialize)]
    struct Count {
        count: i64,
    }

    #[tokio::test]
    async fn event_inserts_are_idempotent() {
        let Some((_database_url, _repo_client)) = setup_postgres().await else {
            return;
        };

        let repo = test_runner::new_repo();
        let pool = repo.get_pool(1).await;
        let mut conn = ChaindexingRepo::get_conn(&pool).await;
        let event = factory::unique_transfer_event_with_contract(
            chaindexing::Contract::new("integration-erc721").add_address(
                factory::BAYC_CONTRACT_ADDRESS,
                &ChainId::Mainnet,
                1,
            ),
        );

        ChaindexingRepo::create_events(&mut conn, std::slice::from_ref(&event)).await;
        ChaindexingRepo::create_events(&mut conn, std::slice::from_ref(&event)).await;

        let repo_client = repo.get_client().await;
        let count: Count = ChaindexingRepo::load_data(
            &repo_client,
            &format!(
                "SELECT COUNT(*)::BIGINT AS count
                 FROM chaindexing_events
                 WHERE id = '{}'",
                event.id
            ),
        )
        .await
        .unwrap();

        assert_eq!(count.count, 1);
    }

    #[derive(Debug, Deserialize)]
    struct CheckpointRow {
        next_block_number: i64,
    }

    #[tokio::test]
    async fn checkpoint_updates_dual_write_and_stream_reads_checkpoint_first() {
        let Some((_database_url, mut repo_client)) = setup_postgres().await else {
            return;
        };
        let suffix = test_runner::generate_unique_test_suffix();
        let contract_name = format!("integration-checkpoint-{suffix}");
        let address = factory::contract_address_for_seed(&format!("checkpoint-{suffix}"));
        let chain_id = ChainId::Mainnet;
        let checkpoint_block_number = 44;
        let stale_cursor_block_number = 11;

        ChaindexingRepo::create_contract_addresses(
            &repo_client,
            &[UnsavedContractAddress::new(
                &contract_name,
                &address,
                &chain_id,
                10,
            )],
        )
        .await;

        let txn_client = ChaindexingRepo::get_txn_client(&mut repo_client).await;
        ChaindexingRepo::update_next_block_number_to_handle_from(
            &txn_client,
            &address,
            chain_id as u64,
            checkpoint_block_number,
        )
        .await;
        ChaindexingRepo::commit_txns(txn_client).await;

        let checkpoint: CheckpointRow = ChaindexingRepo::load_data(
            &repo_client,
            &format!(
                "SELECT next_block_number
                 FROM chaindexing_checkpoints
                 WHERE chain_id = {}
                   AND contract_address = '{}'
                   AND handler_kind = 'reducer'
                   AND handler_id = 'default'",
                chain_id as u64, address
            ),
        )
        .await
        .unwrap();
        assert_eq!(checkpoint.next_block_number, checkpoint_block_number as i64);

        ChaindexingRepo::execute(
            &repo_client,
            &format!(
                "UPDATE chaindexing_contract_addresses
                 SET next_block_number_to_handle_from = {stale_cursor_block_number}
                 WHERE chain_id = {} AND address = '{}'",
                chain_id as u64, address
            ),
        )
        .await;

        let shared_client = Arc::new(Mutex::new(repo_client));
        let stream =
            chaindexing::streams::ContractAddressesStream::new(&shared_client, chain_id as i64)
                .with_chunk_size(1);
        futures_util::pin_mut!(stream);

        let mut streamed_contract = None;
        while let Some(batch) = stream.next().await {
            streamed_contract =
                batch.into_iter().find(|contract_address| contract_address.address == address);

            if streamed_contract.is_some() {
                break;
            }
        }

        let streamed_contract = streamed_contract.unwrap();
        assert_eq!(
            streamed_contract.next_block_number_to_handle_from,
            checkpoint_block_number as i64
        );
    }

    struct SuccessfulDispatcher;

    #[chaindexing::augmenting_std::async_trait]
    impl OutboxDispatcher for SuccessfulDispatcher {
        async fn dispatch(&self, job: OutboxJob) -> Result<(), String> {
            assert_eq!(job.handler_id, "integration-dispatcher");
            Ok(())
        }
    }

    #[derive(Debug, Deserialize)]
    struct OutboxStatus {
        status: String,
        attempt_count: i32,
    }

    #[tokio::test]
    async fn outbox_dispatch_marks_jobs_delivered() {
        let Some((database_url, repo_client)) = setup_postgres().await else {
            return;
        };
        let idempotency_key = format!("integration-{}", test_runner::generate_unique_test_suffix());

        ChaindexingRepo::execute(
            &repo_client,
            &format!(
                "INSERT INTO chaindexing_outbox
                    (idempotency_key, chain_id, contract_address, event_id, handler_id, payload, status)
                 VALUES
                    ('{idempotency_key}', 1, '0xabc', '00000000-0000-0000-0000-000000000000', 'integration-dispatcher', '{{}}'::jsonb, 'pending')"
            ),
        )
        .await;

        let dispatched = dispatch_pending_outbox_jobs(
            &database_url,
            &SuccessfulDispatcher,
            OutboxDispatchConfig {
                batch_size: 1,
                ..Default::default()
            },
        )
        .await;

        let status: OutboxStatus = ChaindexingRepo::load_data(
            &repo_client,
            &format!(
                "SELECT status, attempt_count
                 FROM chaindexing_outbox
                 WHERE idempotency_key = '{idempotency_key}'"
            ),
        )
        .await
        .unwrap();

        assert_eq!(dispatched, 1);
        assert_eq!(status.status, "delivered");
        assert_eq!(status.attempt_count, 1);
    }
}
