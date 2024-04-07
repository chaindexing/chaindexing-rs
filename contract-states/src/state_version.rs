use std::collections::HashMap;

use events::Event;
use repo::{ExecutesWithRawQuery, LoadsDataWithRawQuery, Repo, RepoRawQueryTxnClient};

use crate::state_versions;

use super::{serde_map_to_string_map, to_columns_and_values};

pub fn table_name(state_table_name: &str) -> String {
    format!("{state_versions::TABLE_PREFIX}{state_table_name}")
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
    client: &RepoRawQueryTxnClient<'a>,
) -> HashMap<String, String> {
    let mut state_version = state.clone();
    state_version.insert(
        "state_version_group_id".to_owned(),
        uuid::Uuid::new_v4().to_string(),
    );

    append(&state_version, state_table_name, event, client).await
}

pub async fn update<'a>(
    state: &HashMap<String, String>,
    updates: &HashMap<String, String>,
    state_table_name: &str,
    event: &Event,
    client: &RepoRawQueryTxnClient<'a>,
) -> HashMap<String, String> {
    let mut state_version = state.clone();
    state_version.extend(updates.clone());
    append(&state_version, state_table_name, event, client).await
}

pub async fn delete<'a>(
    state: &HashMap<String, String>,
    state_table_name: &str,
    event: &Event,
    client: &RepoRawQueryTxnClient<'a>,
) -> HashMap<String, String> {
    let mut state_version = state.clone();
    state_version.insert("state_version_is_deleted".to_owned(), "true".to_owned());
    append(&state_version, state_table_name, event, client).await
}

async fn append<'a>(
    partial_state_version: &HashMap<String, String>,
    state_table_name: &str,
    event: &Event,
    client: &RepoRawQueryTxnClient<'a>,
) -> HashMap<String, String> {
    let mut state_version = partial_state_version.clone();
    state_version.extend(Self::extract_part_from_event(event));

    let (columns, values) = to_columns_and_values(&state_version);
    let query = format!(
        "INSERT INTO {table_name} ({columns}) VALUES ({values})
      RETURNING *",
        table_name = table_name(state_table_name),
        columns = columns.join(","),
        values = values.join(",")
    );

    serde_map_to_string_map(
        Repo::load_data_from_raw_query_with_txn_client::<HashMap<String, serde_json::Value>>(
            client, &query,
        )
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
