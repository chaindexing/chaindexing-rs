#[cfg(feature = "postgres")]
mod postgres_repo;

#[doc(hidden)]
#[cfg(feature = "postgres")]
pub use postgres_repo::{
    Conn as PostgresRepoConn, Pool as PostgresRepoPool, PostgresRepo, PostgresRepoAsyncConnection,
    PostgresRepoClient, PostgresRepoTxnClient,
};

mod repo;

#[doc(hidden)]
pub use repo::{ExecutesWithRawQuery, HasRawQueryClient, Repo, RepoError};

#[doc(hidden)]
pub(crate) use repo::{LoadsDataWithRawQuery, Migratable, RepoMigrations, SQLikeMigrations};

#[doc(hidden)]
pub mod streams;
