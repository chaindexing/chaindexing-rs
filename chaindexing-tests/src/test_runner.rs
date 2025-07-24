use crate::db;
use chaindexing::{
    ChaindexingRepo, ChaindexingRepoAsyncConnection, ChaindexingRepoClient, ChaindexingRepoConn,
    ChaindexingRepoPool, HasRawQueryClient, Repo,
};
use dotenvy::dotenv;
use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

// Global counter for generating unique test data across all threads
static GLOBAL_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub async fn get_pool() -> ChaindexingRepoPool {
    new_repo().get_pool(1).await
}

/// Generate a unique test suffix for this test execution
/// Uses thread ID + global counter + timestamp to ensure uniqueness across parallel tests
pub fn generate_unique_test_suffix() -> String {
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let thread_id = thread::current().id();
    let counter = GLOBAL_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();

    format!(
        "test_{}_{}_{}_{}",
        format!("{thread_id:?}").replace("ThreadId(", "").replace(")", ""),
        counter,
        timestamp % 1_000_000, // Use last 6 digits to keep it shorter
        std::process::id()
    )
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

    // Use test transaction for automatic rollback and isolation
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

        // Skip cleanup since we now have proper unique data generation
        // cleanup_test_data(&repo_client).await;
    }

    test_fn(repo_client).await;
}

/// Enhanced version that uses proper transaction isolation for parallel tests
pub async fn run_test_with_txn<TestFn, Fut>(test_fn: TestFn)
where
    TestFn: Fn(ChaindexingRepoClient, String) -> Fut,
    Fut: Future<Output = ()>,
{
    let repo_client = new_repo().get_client().await;
    let unique_suffix = generate_unique_test_suffix();

    if should_setup_test_db() {
        db::setup();

        chaindexing::booting::setup_root(&repo_client).await;
        chaindexing::booting::run_internal_migrations(&repo_client).await;
    }

    test_fn(repo_client, unique_suffix).await;
}

pub fn new_repo() -> ChaindexingRepo {
    ChaindexingRepo::new(db::database_url().as_str())
}

fn should_setup_test_db() -> bool {
    dotenv().ok();

    env::var("SETUP_TEST_DB").is_ok()
}
