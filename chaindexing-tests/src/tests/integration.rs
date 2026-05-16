#[cfg(test)]
mod postgres_integration {
    use std::env;

    use chaindexing::{
        augmenting_std::serde::Deserialize, booting, dispatch_pending_outbox_jobs, ChainId,
        ChaindexingRepo, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery,
        OutboxDispatchConfig, OutboxDispatcher, OutboxJob, Repo,
    };
    use dotenvy::dotenv;

    use crate::db;
    use crate::factory;
    use crate::test_runner;

    async fn setup_postgres() -> Option<(String, chaindexing::ChaindexingRepoClient)> {
        dotenv().ok();
        let database_url = env::var("TEST_DATABASE_URL").ok()?;

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
            eprintln!("skipping postgres integration test; TEST_DATABASE_URL is not set");
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
                   'chaindexing_outbox'
               )",
        )
        .await;
        let table_names: Vec<_> = tables.into_iter().map(|table| table.table_name).collect();

        assert!(table_names.contains(&"chaindexing_events".to_string()));
        assert!(table_names.contains(&"chaindexing_checkpoints".to_string()));
        assert!(table_names.contains(&"chaindexing_blocks".to_string()));
        assert!(table_names.contains(&"chaindexing_outbox".to_string()));
    }

    #[derive(Debug, Deserialize)]
    struct Count {
        count: i64,
    }

    #[tokio::test]
    async fn event_inserts_are_idempotent() {
        let Some((_database_url, _repo_client)) = setup_postgres().await else {
            eprintln!("skipping postgres integration test; TEST_DATABASE_URL is not set");
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

        ChaindexingRepo::create_events(&mut conn, &[event.clone()]).await;
        ChaindexingRepo::create_events(&mut conn, &[event.clone()]).await;

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
            eprintln!("skipping postgres integration test; TEST_DATABASE_URL is not set");
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
        assert_eq!(status.attempt_count, 0);
    }
}
