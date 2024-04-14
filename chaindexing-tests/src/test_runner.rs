use crate::db;
use chaindexing::{
    ChaindexingRepo, ChaindexingRepoAsyncConnection, ChaindexingRepoConn, ChaindexingRepoPool,
    HasRawQueryClient, Repo,
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

        let raw_query_client = new_repo().get_raw_query_client().await;
        chaindexing::booting::run_internal_migrations(&raw_query_client).await;
    }

    conn.begin_test_transaction().await.unwrap();

    test_fn(conn).await;
}

pub fn new_repo() -> ChaindexingRepo {
    ChaindexingRepo::new(db::database_url().as_str())
}

fn should_setup_test_db() -> bool {
    dotenv().ok();

    env::var("SETUP_TEST_DB").is_ok()
}
