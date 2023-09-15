use chaindexing::{ChaindexingRepo, Migratable, Repo};
use chaindexing_tests::db;

#[tokio::main]
async fn main() {
    // Run once to setup database
    // Useful in a CI environment running parallel tests
    db::setup();

    let repo = ChaindexingRepo::new(db::database_url().as_str());
    let pool = repo.get_pool(1).await;
    let mut conn = ChaindexingRepo::get_conn(&pool).await;

    repo.migrate(&mut conn).await;
}
