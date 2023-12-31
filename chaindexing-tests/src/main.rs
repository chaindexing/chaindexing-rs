use chaindexing::{Chaindexing, ChaindexingRepo, HasRawQueryClient, Repo};
use chaindexing_tests::{db, tests};

#[tokio::main]
async fn main() {
    db::setup();
    let repo = ChaindexingRepo::new(db::database_url().as_str());
    let raw_query_client = repo.get_raw_query_client().await;
    Chaindexing::run_internal_migrations(&raw_query_client).await;

    tests::setup().await;
}
