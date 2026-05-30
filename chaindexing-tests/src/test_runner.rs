use crate::db;
use chaindexing::{
    ChaindexingRepo, ChaindexingRepoAsyncConnection, ChaindexingRepoClient, ChaindexingRepoConn,
    ChaindexingRepoPool, HasRawQueryClient, Repo,
};
use dotenvy::dotenv;
use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::OnceCell;

const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";
const ALLOW_DB_TEST_SKIP_ENV: &str = "ALLOW_DB_TEST_SKIP";

// Global counter for generating unique test data across all threads
static GLOBAL_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
static TEST_DB_SETUP: OnceCell<()> = OnceCell::const_new();

pub async fn get_pool() -> ChaindexingRepoPool {
    setup_test_database_if_requested().await;

    new_repo().get_pool(1).await.expect("test database pool")
}

pub fn has_test_database() -> bool {
    dotenv().ok();

    env::var(TEST_DATABASE_URL_ENV).is_ok()
}

pub fn skip_without_test_database() -> bool {
    if has_test_database() {
        false
    } else if should_skip_missing_test_database(env::var("CI").is_ok(), allows_db_test_skip()) {
        eprintln!(
            "skipping postgres-backed test; {TEST_DATABASE_URL_ENV} is not set \
             ({ALLOW_DB_TEST_SKIP_ENV}=1 can opt out explicitly in CI)"
        );
        true
    } else {
        panic!(
            "{TEST_DATABASE_URL_ENV} is not set. CI must provide a Postgres test database or set \
             {ALLOW_DB_TEST_SKIP_ENV}=1 to intentionally skip postgres-backed tests."
        );
    }
}

fn allows_db_test_skip() -> bool {
    matches!(
        env::var(ALLOW_DB_TEST_SKIP_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn should_skip_missing_test_database(running_in_ci: bool, explicit_allow: bool) -> bool {
    explicit_allow || !running_in_ci
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
    if skip_without_test_database() {
        return;
    }

    setup_test_database_if_requested().await;

    let mut conn = ChaindexingRepo::get_conn(pool).await.expect("test database connection");

    // Use test transaction for automatic rollback and isolation
    conn.begin_test_transaction().await.unwrap();

    test_fn(conn).await;
}

pub async fn run_test_new<TestFn, Fut>(test_fn: TestFn)
where
    TestFn: Fn(ChaindexingRepoClient) -> Fut,
    Fut: Future<Output = ()>,
{
    if skip_without_test_database() {
        return;
    }

    setup_test_database_if_requested().await;

    let repo_client = new_repo().get_client().await.expect("test database client");
    test_fn(repo_client).await;
}

/// Enhanced version that uses proper transaction isolation for parallel tests
pub async fn run_test_with_txn<TestFn, Fut>(test_fn: TestFn)
where
    TestFn: Fn(ChaindexingRepoClient, String) -> Fut,
    Fut: Future<Output = ()>,
{
    if skip_without_test_database() {
        return;
    }

    setup_test_database_if_requested().await;

    let repo_client = new_repo().get_client().await.expect("test database client");
    let unique_suffix = generate_unique_test_suffix();

    test_fn(repo_client, unique_suffix).await;
}

pub fn new_repo() -> ChaindexingRepo {
    ChaindexingRepo::new(db::database_url().as_str())
}

fn should_setup_test_db() -> bool {
    dotenv().ok();

    env::var("SETUP_TEST_DB").is_ok()
}

pub async fn setup_test_database_if_requested() {
    if !should_setup_test_db() {
        return;
    }

    TEST_DB_SETUP
        .get_or_init(|| async {
            db::setup();

            let repo_client = new_repo().get_client().await.expect("test database client");
            chaindexing::booting::setup_root(&repo_client).await.expect("setup root tables");
            chaindexing::booting::run_internal_migrations(&repo_client)
                .await
                .expect("setup internal tables");
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_test_database_is_skipped_locally() {
        assert!(should_skip_missing_test_database(false, false));
    }

    #[test]
    fn missing_test_database_fails_ci_without_explicit_opt_out() {
        assert!(!should_skip_missing_test_database(true, false));
    }

    #[test]
    fn missing_test_database_can_be_explicitly_skipped_in_ci() {
        assert!(should_skip_missing_test_database(true, true));
    }
}
