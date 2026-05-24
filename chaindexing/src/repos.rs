#[cfg(feature = "postgres")]
mod postgres_repo;

#[doc(hidden)]
#[cfg(feature = "postgres")]
pub use postgres_repo::{
    Conn as PostgresRepoConn, Pool as PostgresRepoPool, PostgresRepo, PostgresRepoAsyncConnection,
    PostgresRepoClient, PostgresRepoTxnClient, PostgresTlsConfig, PostgresTlsMode,
};

mod repo;

#[doc(hidden)]
pub use repo::{ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, Repo, RepoError};

#[doc(hidden)]
pub(crate) use repo::{Migratable, RepoMigrations, SQLikeMigrations};

#[doc(hidden)]
pub mod streams;
