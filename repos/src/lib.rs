#[cfg(feature = "postgres")]
pub use postgres_repo::{PostgresRepo, PostgresRepoRawQueryClient};

#[cfg(feature = "postgres")]
pub use repos::{PostgresRepo, PostgresRepoConn, PostgresRepoPool};

#[cfg(feature = "postgres")]
pub type Repo = PostgresRepo;

#[cfg(feature = "postgres")]
pub type RepoPool = PostgresRepoPool;

#[cfg(feature = "postgres")]
pub type RepoConn<'a> = PostgresRepoConn<'a>;

#[cfg(feature = "postgres")]
pub type RepoRawQueryClient = PostgresRepoRawQueryClient;

#[cfg(feature = "postgres")]
pub use postgres_repo::PostgresRepoAsyncConnection as RepoAsyncConnection;
