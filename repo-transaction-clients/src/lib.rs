#[cfg(feature = "postgres")]
pub type PostgresRepoRawQueryTxnClient<'a> = tokio_postgres::Transaction<'a>;

#[cfg(feature = "postgres")]
pub type RepoRawQueryTxnClient<'a> = PostgresRepoRawQueryTxnClient<'a>;
