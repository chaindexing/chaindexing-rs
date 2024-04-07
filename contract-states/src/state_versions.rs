use std::collections::HashMap;

use events::Event;
use repos::{ExecutesWithRawQuery, LoadsDataWithRawQuery, Repo, RepoRawQueryTxnClient};

use crate::state_version;

use super::{serde_map_to_string_map, to_columns_and_values};

pub const TABLE_PREFIX: &str = "chaindexing_state_versions_for_";
pub const UNIQUE_FIELDS: [&str; 2] = ["state_version_id", "state_version_is_deleted"];

pub async fn get<'a>(
    from_block_number: i64,
    chain_id: i64,
    state_table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) -> Vec<HashMap<String, String>> {
    let query = format!(
        "SELECT * FROM {table_name} 
            WHERE chain_id = {chain_id}
            AND block_number >= {from_block_number}",
        table_name = state_version::table_name(state_table_name),
    );

    Repo::load_data_list_from_raw_query_with_txn_client::<HashMap<String, serde_json::Value>>(
        client, &query,
    )
    .await
    .into_iter()
    .map(serde_map_to_string_map)
    .collect()
}

pub fn get_ids(state_versions: &[HashMap<String, String>]) -> Vec<String> {
    state_versions
        .iter()
        .map(|state_version| state_version.get("state_version_id").unwrap())
        .cloned()
        .collect()
}

pub fn get_group_ids(state_versions: &[HashMap<String, String>]) -> Vec<String> {
    state_versions
        .iter()
        .map(|state_version| state_version.get("state_version_group_id").unwrap())
        .cloned()
        .collect()
}

pub async fn delete_by_ids<'a>(
    ids: &[String],
    state_table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) {
    let query = format!(
        "DELETE FROM {table_name}
            WHERE state_version_id IN ({ids})",
        table_name = state_version::table_name(state_table_name),
        ids = ids.join(",")
    );

    Repo::execute_raw_query_in_txn(client, &query).await;
}

pub async fn get_latest<'a>(
    group_ids: &[String],
    state_table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) -> Vec<HashMap<String, String>> {
    let query = format!(
        "SELECT DISTINCT ON (state_version_group_id) * FROM {table_name} 
            WHERE state_version_group_id IN ({group_ids}) 
            ORDER BY state_version_group_id, block_number, log_index DESC",
        table_name = state_version::table_name(state_table_name),
        group_ids = group_ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(",")
    );

    Repo::load_data_list_from_raw_query_with_txn_client::<HashMap<String, serde_json::Value>>(
        client, &query,
    )
    .await
    .into_iter()
    .map(serde_map_to_string_map)
    .collect()
}
