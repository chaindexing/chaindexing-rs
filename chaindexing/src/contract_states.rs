use std::{collections::HashMap, fmt::Debug};

use crate::{
    ChaindexingRepo, ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, LoadsDataWithRawQuery,
};
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub enum ContractStateError {
    AlreadyInserted,
    NotFound,
}

const IMMUTABLE_TABLE_NAME_PREFIX: &'static str = "chaindexing_";

#[async_trait::async_trait]
pub trait ContractState:
    Into<HashMap<&'static str, String>> + Debug + Sync + Send + Clone + DeserializeOwned + 'static
{
    fn table_name() -> &'static str;

    fn immutable_table_name() -> String {
        let mut table_name = IMMUTABLE_TABLE_NAME_PREFIX.to_string();
        table_name.push_str(Self::table_name());
        table_name
    }

    async fn read(
        client: &ChaindexingRepoRawQueryClient,
        filters: HashMap<&'static str, String>,
    ) -> Result<Option<Self>, ContractStateError>
    where
        Self: Sized,
    {
        let states = Self::read_many(client, filters).await?;
        Ok(states.first().cloned())
    }

    async fn read_many(
        client: &ChaindexingRepoRawQueryClient,
        filters: HashMap<&'static str, String>,
    ) -> Result<Vec<Self>, ContractStateError>
    where
        Self: Sized,
    {
        let filters = filters.iter().fold(vec![], |mut filters, (column, value)| {
            filters.push(format!("{} = '{}'", column, value));

            filters
        });
        let filters = filters.join(" AND ");

        // TODO: Move to Repo level and prevent SQLInjection
        let raw_query = format!(
            "SELECT FROM {table_name} WHERE {filters}",
            table_name = Self::table_name(),
        );

        let states: Vec<Self> =
            ChaindexingRepo::load_data_list_from_raw_query(client, &raw_query).await;

        Ok(states)
    }

    async fn create(
        &self,
        client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        let unsaved_state: HashMap<&str, String> = self.clone().into();
        Self::append_row(unsaved_state, client, false).await?;
        Ok(())
    }

    async fn update(
        &self,
        client: &ChaindexingRepoRawQueryClient,
        updates: HashMap<&'static str, String>,
    ) -> Result<(), ContractStateError> {
        let mut state: HashMap<&str, String> = self.clone().into();
        state.extend(updates);
        let state: HashMap<&str, String> = self.clone().into();
        Self::append_row(state, client, false).await?;
        Ok(())
    }

    async fn delete(
        &self,
        client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        let state: HashMap<&str, String> = self.clone().into();
        Self::append_row(state, client, true).await?;
        Ok(())
    }

    async fn append_row(
        state: HashMap<&str, String>,
        client: &ChaindexingRepoRawQueryClient,
        is_deleted: bool,
    ) -> Result<(), ContractStateError> {
        let (columns, values) = state.into_iter().fold(
            (vec!["is_deleted"], vec![is_deleted.to_string()]),
            |(mut columns, mut values), (column, value)| {
                columns.push(column);
                values.push(format!("'{}'", value));

                (columns, values)
            },
        );
        let columns = columns.join(",");
        let values = values.join(",");

        // TODO: Move to Repo level and prevent SQLInjection
        let raw_query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            table_name = Self::table_name(),
            columns = columns,
            values = values
        );
        ChaindexingRepo::execute_raw_query(client, raw_query.as_str()).await;

        Ok(())
    }
}

// Since contract states are rebuildable from ground up, we can
// easen the type strictness for consumer applications.
pub trait ContractStateMigrations: Send + Sync {
    fn migrations(&self) -> Vec<&'static str>;

    // TODO: Featurize the Create Table part
    fn get_migrations(&self) -> Vec<String> {
        self.migrations()
            .iter()
            .flat_map(|user_migration| {
                if user_migration.starts_with("CREATE TABLE IF NOT EXISTS") {
                    let create_state_table_migration =
                        append_block_fields_migration(&user_migration);
                    let create_immutable_state_table_migration =
                        append_block_fields_migration(&user_migration).replace(
                            "CREATE TABLE IF NOT EXISTS ",
                            format!("CREATE TABLE IF NOT EXISTS {}", IMMUTABLE_TABLE_NAME_PREFIX)
                                .as_str(),
                        );

                    vec![
                        create_state_table_migration,
                        create_immutable_state_table_migration,
                    ]
                } else {
                    vec![user_migration.to_string()]
                }
            })
            .collect()
    }
}

// TODO: Featurize this to support non-sqlike DB
fn append_block_fields_migration(migration: &str) -> String {
    let mut migration = migration.replace("\n", "");
    migration.push_str(block_fields_migration());
    migration
        .split_ascii_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .replace(" ),", " ,")
}

fn block_fields_migration() -> &'static str {
    "block_hash TEXT NOT NULL,
    block_number BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index BIGINT NOT NULL,
    log_index BIGINT NOT NULL)"
}

#[cfg(test)]
mod contract_state_migrations_get_migration_test {
    use super::*;

    #[test]
    fn returns_two_table_migrations_for_create_state_migrations() {
        let contract_state = test_contract_state();

        assert_eq!(
            contract_state.get_migrations().len(),
            contract_state.migrations().len() + 1
        );
    }

    #[test]
    fn appends_block_fields_migrations_to_create_state_migrations() {
        let contract_state = test_contract_state();
        let migrations = contract_state.get_migrations();
        let state_migration = migrations.first().unwrap();

        assert_block_fields_in_migration(state_migration);
    }

    #[test]
    fn creates_an_extra_migration_for_an_immutable_state_equivalent() {
        let contract_state = test_contract_state();
        let mut migrations = contract_state.get_migrations();
        migrations.pop();
        let immutable_state_migration = migrations.last().unwrap();

        assert!(immutable_state_migration.contains(IMMUTABLE_TABLE_NAME_PREFIX));
        assert_block_fields_in_migration(immutable_state_migration);
    }

    fn assert_block_fields_in_migration(migration: &str) {
        const BLOCK_FIELDS: [&'static str; 5] = [
            "block_hash",
            "block_number",
            "transaction_hash",
            "transaction_index",
            "log_index",
        ];
        BLOCK_FIELDS.iter().for_each(|field| assert!(migration.contains(field)));
    }

    #[test]
    fn returns_other_migrations_untouched() {
        let contract_state = test_contract_state();

        assert_eq!(
            contract_state.migrations().last().unwrap(),
            contract_state.get_migrations().last().unwrap()
        );
    }

    fn test_contract_state() -> impl ContractStateMigrations {
        struct TestContractState;

        impl ContractStateMigrations for TestContractState {
            fn migrations(&self) -> Vec<&'static str> {
                vec![
                    "CREATE TABLE IF NOT EXISTS nft_states (
                        token_id INTEGER NOT NULL,
                        contract_address TEXT NOT NULL,
                        owner_address TEXT NOT NULL,
                        is_deleted BOOLEAN NOT NULL DEFAULT false,
                        inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW() 
                    )",
                    "UPDATE nft_states
                    SET owner_address = ''
                    WHERE owner_address IS NULL",
                ]
            }
        }

        TestContractState
    }
}
