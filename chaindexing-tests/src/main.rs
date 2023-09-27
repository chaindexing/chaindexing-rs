use chaindexing::{Chaindexing, ChaindexingRepo, HasRawQueryClient, Repo};
use chaindexing_tests::db;

#[tokio::main]
async fn main() {
    // Run once to setup database
    // Useful in a CI environment running parallel tests
    db::setup();
    let repo = ChaindexingRepo::new(db::database_url().as_str());
    let raw_query_client = repo.get_raw_query_client().await;
    Chaindexing::run_internal_migrations(&raw_query_client).await;
}
