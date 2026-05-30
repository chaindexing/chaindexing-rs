use chaindexing::{ChaindexingRepo, HasRawQueryClient};
use chaindexing_tests::{db, tests};

#[tokio::main]
async fn main() {
    db::setup();
    let repo = ChaindexingRepo::new(db::database_url().as_str());
    let repo_client = repo.get_client().await.expect("test database client");
    chaindexing::booting::setup_root(&repo_client).await.expect("setup root tables");
    chaindexing::booting::run_internal_migrations(&repo_client)
        .await
        .expect("setup internal tables");

    tests::setup().await;
}
