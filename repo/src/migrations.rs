use crate::ExecutesWithRawQuery;

pub trait RepoMigrations: Migratable {
    fn create_nodes_migration() -> &'static [&'static str];
    fn create_contract_addresses_migration() -> &'static [&'static str];
    fn drop_contract_addresses_migration() -> &'static [&'static str];
    fn create_events_migration() -> &'static [&'static str];
    fn drop_events_migration() -> &'static [&'static str];
    fn create_reset_counts_migration() -> &'static [&'static str];
    fn create_reorged_blocks_migration() -> &'static [&'static str];
    fn drop_reorged_blocks_migration() -> &'static [&'static str];

    fn get_internal_migrations() -> Vec<&'static str> {
        [
            Self::create_contract_addresses_migration(),
            Self::create_events_migration(),
            Self::create_reorged_blocks_migration(),
        ]
        .concat()
    }

    fn get_reset_internal_migrations() -> Vec<&'static str> {
        [
            Self::drop_contract_addresses_migration(),
            Self::drop_events_migration(),
            Self::drop_reorged_blocks_migration(),
        ]
        .concat()
    }
}

#[async_trait::async_trait]
pub trait Migratable: ExecutesWithRawQuery + Sync + Send {
    async fn migrate(client: &Self::RawQueryClient, migrations: Vec<impl AsRef<str> + Send + Sync>)
    where
        Self: Sized,
    {
        for migration in migrations {
            Self::execute_raw_query(client, migration.as_ref()).await;
        }
    }
}

pub struct SQLikeMigrations;

impl SQLikeMigrations {
    pub fn create_nodes() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_nodes (
              id SERIAL PRIMARY KEY,
              last_active_at BIGINT DEFAULT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT,
              inserted_at BIGINT DEFAULT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT
      )"]
    }

    pub fn create_contract_addresses() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_contract_addresses (
              id SERIAL PRIMARY KEY,
              address VARCHAR NOT NULL,
              contract_name VARCHAR NOT NULL,
              chain_id BIGINT NOT NULL,
              start_block_number BIGINT NOT NULL,
              next_block_number_to_ingest_from BIGINT NOT NULL,
              next_block_number_to_handle_from BIGINT NOT NULL
      )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_contract_addresses_chain_address_index
      ON chaindexing_contract_addresses(chain_id, address)",
        ]
    }
    pub fn drop_contract_addresses() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_contract_addresses"]
    }

    pub fn create_events() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_events (
              id uuid PRIMARY KEY,
              chain_id BIGINT NOT NULL,
              contract_address VARCHAR NOT NULL,
              contract_name VARCHAR NOT NULL,
              abi TEXT NOT NULL,
              log_params JSON NOT NULL,
              parameters JSON NOT NULL,
              topics JSON NOT NULL,
              block_hash VARCHAR NOT NULL,
              block_number BIGINT NOT NULL,
              block_timestamp BIGINT NOT NULL,
              transaction_hash VARCHAR NOT NULL,
              transaction_index INTEGER NOT NULL,
              log_index INTEGER NOT NULL,
              removed BOOLEAN NOT NULL,
              inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
          )",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_chain_contract_block_log_index
          ON chaindexing_events(chain_id,contract_address,block_number,log_index)",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_abi
          ON chaindexing_events(abi)",
        ]
    }
    pub fn drop_events() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_events"]
    }

    pub fn create_reorged_blocks() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_reorged_blocks (
              id SERIAL PRIMARY KEY,
              chain_id BIGINT NOT NULL,
              block_number BIGINT NOT NULL,
              handled_at TIMESTAMPTZ,
              inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
          )"]
    }
    pub fn drop_reorged_blocks() -> &'static [&'static str] {
        &["DROP TABLE IF EXISTS chaindexing_reorged_blocks"]
    }

    pub fn create_reset_counts() -> &'static [&'static str] {
        &["CREATE TABLE IF NOT EXISTS chaindexing_reset_counts (
              id BIGSERIAL PRIMARY KEY,
              inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
          )"]
    }
}
