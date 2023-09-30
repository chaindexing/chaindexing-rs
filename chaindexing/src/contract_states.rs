use std::{collections::HashMap, fmt::Debug};

mod migrations;

use crate::Event;
use crate::{
    ChaindexingRepo, ChaindexingRepoRawQueryTxnClient, ExecutesWithRawQuery, LoadsDataWithRawQuery,
};
pub use migrations::ContractStateMigrations;
use serde::de::DeserializeOwned;
use serde::Serialize;

// TODO:
// Investigate HashMap Interface Vs Json (Serde)
// Move Queries to Repo level and prevent SQLInjection
// Create Into<StateVersionEvent> to extract events fields we care about once and avoid passing around the whole Event struct

#[derive(Debug)]
pub enum ContractStateError {
    AlreadyInserted,
    NotFound,
}

#[async_trait::async_trait]
pub trait ContractState:
    DeserializeOwned + Serialize + Debug + Sync + Send + Clone + 'static
{
    fn table_name() -> &'static str;

    async fn read_one<'a>(
        filters: HashMap<String, String>,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<Option<Self>, ContractStateError>
    where
        Self: Sized,
    {
        let states = Self::read_many(filters, event, client).await?;
        Ok(states.first().cloned())
    }

    async fn read_many<'a>(
        filters: HashMap<String, String>,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<Vec<Self>, ContractStateError>
    where
        Self: Sized,
    {
        match Self::get_state_fields(client).await {
            None => Ok(vec![]),
            Some(state_fields) => {
                let raw_query = format!(
                    "SELECT DISTINCT ON ({state_fields_by_comma}) * FROM {table_name}
                    WHERE {filters} 
                    AND state_version_block_number <= {block_number}
                    AND state_version_log_index < {log_index}
                    ORDER BY {state_fields_by_comma},state_version_block_number DESC",
                    state_fields_by_comma = state_fields.join(","),
                    table_name = StateVersions::table_name(Self::table_name()),
                    filters = to_and_filters(&filters),
                    block_number = event.block_number,
                    log_index = event.log_index
                );

                let states: Vec<Self> =
                    ChaindexingRepo::load_data_list_from_raw_query_with_txn_client(
                        client, &raw_query,
                    )
                    .await;

                Ok(states)
            }
        }
    }

    async fn get_state_fields<'a>(
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Option<Vec<String>> {
        Self::get_random_state(client)
            .await
            .and_then(|random_state| Some(random_state.to_map().keys().cloned().collect()))
    }

    async fn get_random_state<'a>(client: &ChaindexingRepoRawQueryTxnClient<'a>) -> Option<Self> {
        let table_name = Self::table_name();
        let query = format!("SELECT * from {table_name} limit 1");

        ChaindexingRepo::load_data_from_raw_query_with_txn_client(client, &query).await
    }

    fn to_map(&self) -> HashMap<String, String> {
        let state: serde_json::Value = serde_json::to_value(self).unwrap();

        let map: HashMap<String, serde_json::Value> = serde_json::from_value(state).unwrap();

        serde_map_to_string_map(map)
    }

    // Every operation should respect the Event context
    async fn create<'a>(
        &self,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<(), ContractStateError> {
        let state = self.to_map();
        StateVersions::create(&state, &Self::table_name(), &event, client).await;
        Self::refresh_state_from_lastest_version(&state, event, client).await;
        Ok(())
    }
    async fn update<'a>(
        &self,
        updates: HashMap<String, String>,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<(), ContractStateError> {
        let state = self.to_map();
        StateVersions::update(&state, &updates, &Self::table_name(), event, client).await;
        Self::refresh_state_from_lastest_version(&state, event, client).await;
        Ok(())
    }
    async fn delete<'a>(
        &self,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Result<(), ContractStateError> {
        let state = self.to_map();
        StateVersions::delete(&state, &Self::table_name(), event, client).await;
        Self::refresh_state_from_lastest_version(&state, event, client).await;
        Ok(())
    }

    async fn refresh_state_from_lastest_version<'a>(
        state: &HashMap<String, String>,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let latest_state_version =
            StateVersions::get_latest(state, &Self::table_name(), event, client).await;
        let latest_state = Self::extract_state_from_version(state, &latest_state_version);
        Self::refresh_state(&state, &latest_state, client).await;
    }
    fn extract_state_from_version(
        state: &HashMap<String, String>,
        state_version: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        state.keys().into_iter().fold(HashMap::new(), |mut state, key| {
            state.insert(key.to_owned(), state_version.get(key).unwrap().to_owned());

            state
        })
    }
    async fn refresh_state<'a>(
        old_state: &HashMap<String, String>,
        new_state: &HashMap<String, String>,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        Self::delete_old_state(old_state, client).await;
        Self::insert_new_state(new_state, client).await;
    }
    async fn delete_old_state<'a>(
        old_state: &HashMap<String, String>,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let query = format!(
            "DELETE FROM {table_name} WHERE {filters}",
            table_name = Self::table_name(),
            filters = to_and_filters(old_state)
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }
    async fn insert_new_state<'a>(
        new_state: &HashMap<String, String>,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let (columns, values) = to_columns_and_values(&new_state);
        let query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            table_name = Self::table_name(),
            columns = columns.join(","),
            values = values.join(",")
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }
}

pub struct StateVersions;

const STATE_VERSIONS_TABLE_PREFIX: &'static str = "chaindexing_state_versions_for_";

impl StateVersions {
    fn table_name(state_table_name: &str) -> String {
        let mut table_name = STATE_VERSIONS_TABLE_PREFIX.to_string();
        table_name.push_str(state_table_name);
        table_name
    }

    async fn get_latest<'a>(
        state: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let query = format!(
            "SELECT * FROM {table_name} 
            WHERE {filters} 
            AND state_version_block_number <= {block_number}
            AND state_version_log_index <= {log_index}
            ORDER BY state_version_block_number, state_version_log_index
            LIMIT 1",
            table_name = Self::table_name(&state_table_name),
            filters = to_and_filters(state),
            block_number = event.block_number,
            log_index = event.log_index
        );

        serde_map_to_string_map(
            ChaindexingRepo::load_data_from_raw_query_with_txn_client::<
                HashMap<String, serde_json::Value>,
            >(client, &query)
            .await
            .unwrap(),
        )
    }
    pub async fn create<'a>(
        state_version: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        Self::append(&state_version, state_table_name, event, client).await;
    }
    pub async fn update<'a>(
        state_version: &HashMap<String, String>,
        updates: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let mut state_version = state_version.clone();
        state_version.extend(updates.clone());
        Self::append(&state_version, state_table_name, event, client).await;
    }
    pub async fn delete<'a>(
        state_version: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let mut state_version = state_version.clone();
        state_version.insert("state_version_is_deleted".to_owned(), "true".to_owned());
        Self::append(&state_version, state_table_name, event, client).await;
    }
    async fn append<'a>(
        state_version: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let mut state_version = state_version.clone();

        state_version.extend(Self::extract_part_from_event(event));
        let (columns, values) = to_columns_and_values(&state_version);
        let query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            table_name = Self::table_name(state_table_name),
            columns = columns.join(","),
            values = values.join(",")
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }
    fn extract_part_from_event(event: &Event) -> HashMap<String, String> {
        // TODO: Add ChainID
        HashMap::from([
            (
                "state_version_contract_address".to_string(),
                event.contract_address.to_owned(),
            ),
            (
                "state_version_transaction_hash".to_string(),
                event.transaction_hash.to_owned(),
            ),
            (
                "state_version_transaction_index".to_string(),
                event.transaction_index.to_string(),
            ),
            (
                "state_version_log_index".to_string(),
                event.log_index.to_string(),
            ),
            (
                "state_version_block_number".to_string(),
                event.block_number.to_string(),
            ),
            (
                "state_version_block_hash".to_string(),
                event.block_hash.to_owned(),
            ),
        ])
    }
}

fn to_columns_and_values(state: &HashMap<String, String>) -> (Vec<String>, Vec<String>) {
    state.into_iter().fold(
        (vec![], vec![]),
        |(mut columns, mut values), (column, value)| {
            columns.push(column.to_string());
            values.push(format!("'{value}'"));

            (columns, values)
        },
    )
}

fn to_and_filters(state: &HashMap<String, String>) -> String {
    let filters = state.iter().fold(vec![], |mut filters, (column, value)| {
        filters.push(format!("{column} = '{value}'"));

        filters
    });

    filters.join(" AND ")
}

fn serde_map_to_string_map(
    serde_map: HashMap<String, serde_json::Value>,
) -> HashMap<String, String> {
    serde_map.iter().fold(HashMap::new(), |mut map, (key, value)| {
        map.insert(key.to_owned(), value.to_string().replace("\"", ""));

        map
    })
}
