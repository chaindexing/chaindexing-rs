use std::collections::HashMap;

pub use crate::event_handlers::{EventHandlerContext, UseEventHandlerContext};
use crate::Event;
use crate::{
    ChaindexingRepo, ChaindexingRepoRawQueryTxnClient, ExecutesWithRawQuery, LoadsDataWithRawQuery,
};

use super::{serde_map_to_string_map, to_columns_and_values};

pub const STATE_VERSIONS_TABLE_PREFIX: &'static str = "chaindexing_state_versions_for_";
pub const STATE_VERSIONS_UNIQUE_FIELDS: [&str; 2] =
    ["state_version_id", "state_version_is_deleted"];

pub struct StateVersions;

impl StateVersions {
    pub async fn get<'a>(
        from_block_number: i64,
        chain_id: i32,
        state_table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Vec<HashMap<String, String>> {
        let query = format!(
            "SELECT * FROM {table_name} 
            WHERE chain_id = {chain_id}
            WHERE block_number >= {from_block_number}",
            table_name = StateVersion::table_name(&state_table_name),
        );

        ChaindexingRepo::load_data_list_from_raw_query_with_txn_client::<
            HashMap<String, serde_json::Value>,
        >(client, &query)
        .await
        .into_iter()
        .map(serde_map_to_string_map)
        .collect()
    }

    pub fn get_ids(state_versions: &Vec<HashMap<String, String>>) -> Vec<String> {
        state_versions
            .iter()
            .map(|state_version| state_version.get("state_version_id").unwrap())
            .cloned()
            .collect()
    }

    pub fn get_group_ids(state_versions: &Vec<HashMap<String, String>>) -> Vec<String> {
        state_versions
            .iter()
            .map(|state_version| state_version.get("state_version_group_id").unwrap())
            .cloned()
            .collect()
    }

    pub async fn delete_by_ids<'a>(
        ids: &Vec<String>,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let query = format!(
            "SELECT * FROM {table_name} 
            WHERE id IN ({ids})",
            ids = ids.join(",")
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }

    pub async fn get_latest<'a>(
        group_ids: &Vec<String>,
        state_table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Vec<HashMap<String, String>> {
        let query = format!(
            "SELECT DISTINCT ON (state_version_group_id) * FROM {table_name} 
            WHERE group_id IN ({group_ids}) 
            ORDER BY state_version_group_id, block_number, log_index DESC",
            table_name = StateVersion::table_name(&state_table_name),
            group_ids = group_ids.join(",")
        );

        ChaindexingRepo::load_data_list_from_raw_query_with_txn_client::<
            HashMap<String, serde_json::Value>,
        >(client, &query)
        .await
        .into_iter()
        .map(serde_map_to_string_map)
        .collect()
    }
}

pub struct StateVersion;

impl StateVersion {
    pub fn table_name(state_table_name: &str) -> String {
        let mut table_name = STATE_VERSIONS_TABLE_PREFIX.to_string();
        table_name.push_str(state_table_name);
        table_name
    }

    pub fn was_deleted(state_version: &HashMap<String, String>) -> bool {
        state_version.get("state_version_is_deleted").unwrap() == "true"
    }

    pub fn get_group_id(state_version: &HashMap<String, String>) -> String {
        state_version.get("state_version_group_id").unwrap().to_owned()
    }

    pub async fn create<'a>(
        state: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let mut state_version = state.clone();
        state_version.insert(
            "state_version_group_id".to_owned(),
            uuid::Uuid::new_v4().to_string(),
        );

        Self::append(&state_version, state_table_name, event, client).await
    }

    pub async fn update<'a>(
        state: &HashMap<String, String>,
        updates: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let mut state_version = state.clone();
        state_version.extend(updates.clone());
        Self::append(&state_version, state_table_name, event, client).await
    }

    pub async fn delete<'a>(
        state: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let mut state_version = state.clone();
        state_version.insert("is_deleted".to_owned(), "true".to_owned());
        Self::append(&state_version, state_table_name, event, client).await
    }

    async fn append<'a>(
        partial_state_version: &HashMap<String, String>,
        state_table_name: &str,
        event: &Event,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let mut state_version = partial_state_version.clone();
        state_version.extend(Self::extract_part_from_event(event));

        let (columns, values) = to_columns_and_values(&state_version);
        let query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})
            RETURNING *",
            table_name = Self::table_name(state_table_name),
            columns = columns.join(","),
            values = values.join(",")
        );

        serde_map_to_string_map(
            ChaindexingRepo::load_data_from_raw_query_with_txn_client::<
                HashMap<String, serde_json::Value>,
            >(client, &query)
            .await
            .unwrap(),
        )
    }

    fn extract_part_from_event(event: &Event) -> HashMap<String, String> {
        HashMap::from([
            (
                "contract_address".to_string(),
                event.contract_address.to_owned(),
            ),
            ("chain_id".to_string(), event.chain_id.to_string()),
            (
                "transaction_hash".to_string(),
                event.transaction_hash.to_owned(),
            ),
            (
                "transaction_index".to_string(),
                event.transaction_index.to_string(),
            ),
            ("log_index".to_string(), event.log_index.to_string()),
            ("block_number".to_string(), event.block_number.to_string()),
            ("block_hash".to_string(), event.block_hash.to_owned()),
        ])
    }
}
