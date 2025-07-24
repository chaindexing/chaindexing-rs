use crate::db;
use chaindexing::{
    ChaindexingRepo, ChaindexingRepoAsyncConnection, ChaindexingRepoClient, ChaindexingRepoConn,
    ChaindexingRepoPool, ExecutesWithRawQuery, HasRawQueryClient, Repo,
};
use dotenvy::dotenv;
use std::env;
use std::future::Future;

pub async fn get_pool() -> ChaindexingRepoPool {
    new_repo().get_pool(1).await
}

pub async fn run_test<'a, TestFn, Fut>(pool: &'a ChaindexingRepoPool, test_fn: TestFn)
where
    TestFn: Fn(ChaindexingRepoConn<'a>) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut conn = ChaindexingRepo::get_conn(pool).await;

    if should_setup_test_db() {
        db::setup();

        let repo_client = new_repo().get_client().await;
        chaindexing::booting::setup_root(&repo_client).await;
        chaindexing::booting::run_internal_migrations(&repo_client).await;
    }

    conn.begin_test_transaction().await.unwrap();

    test_fn(conn).await;
}

pub async fn run_test_new<TestFn, Fut>(test_fn: TestFn)
where
    TestFn: Fn(ChaindexingRepoClient) -> Fut,
    Fut: Future<Output = ()>,
{
    let repo_client = new_repo().get_client().await;

    if should_setup_test_db() {
        db::setup();

        chaindexing::booting::setup_root(&repo_client).await;
        chaindexing::booting::run_internal_migrations(&repo_client).await;

        truncate_all_tables(&repo_client).await;
    }

    test_fn(repo_client).await;
}

pub fn new_repo() -> ChaindexingRepo {
    ChaindexingRepo::new(db::database_url().as_str())
}

fn should_setup_test_db() -> bool {
    dotenv().ok();

    env::var("SETUP_TEST_DB").is_ok()
}

const ALL_TABLE_NAMES: [&str; 5] = [
    "chaindexing_contract_addresses",
    "chaindexing_events",
    "chaindexing_reorged_blocks",
    "chaindexing_root_states",
    "nfts",
];

async fn truncate_all_tables(repo_client: &ChaindexingRepoClient) {
    // First, truncate the core tables
    for table_name in ALL_TABLE_NAMES {
        ChaindexingRepo::execute(
            repo_client,
            &format!("DO $$
            BEGIN
                IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '{table_name}') THEN
                    EXECUTE 'TRUNCATE TABLE {table_name} CASCADE';
                END IF;
            END $$"),
        )
        .await;
    }

    // Then, truncate all dynamically generated state tables using proper syntax
    ChaindexingRepo::execute(
        repo_client,
        "DO $$
        DECLARE
            table_record RECORD;
        BEGIN
            FOR table_record IN
                SELECT table_name 
                FROM information_schema.tables 
                WHERE table_schema = 'public' 
                AND (table_name LIKE 'chaindexing_state_versions_for_%' 
                     OR table_name LIKE '%_nfts' 
                     OR table_name LIKE 'unique_%')
            LOOP
                EXECUTE 'TRUNCATE TABLE ' || table_record.table_name || ' CASCADE';
            END LOOP;
        EXCEPTION
            WHEN OTHERS THEN
                -- Ignore errors if table doesn't exist
                NULL;
        END $$",
    )
    .await;

    // Drop any unique indexes to avoid constraint issues
    ChaindexingRepo::execute(
        repo_client,
        "DO $$
        DECLARE
            index_record RECORD;
        BEGIN
            FOR index_record IN
                SELECT indexname 
                FROM pg_indexes 
                WHERE schemaname = 'public' 
                AND indexname LIKE 'unique_chaindexing_state_versions_for_%'
            LOOP
                EXECUTE 'DROP INDEX IF EXISTS ' || index_record.indexname;
            END LOOP;
        END $$",
    )
    .await;

    // Reset sequences to ensure consistent ID generation
    ChaindexingRepo::execute(
        repo_client,
        "DO $$
        DECLARE
            seq_record RECORD;
        BEGIN
            FOR seq_record IN
                SELECT sequence_name 
                FROM information_schema.sequences 
                WHERE sequence_schema = 'public'
            LOOP
                EXECUTE 'ALTER SEQUENCE ' || seq_record.sequence_name || ' RESTART WITH 1';
            END LOOP;
        END $$",
    )
    .await;
}
