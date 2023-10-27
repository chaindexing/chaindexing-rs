use std::collections::HashMap;

pub use crate::event_handlers::EventHandlerContext;
use crate::{
    ChaindexingRepo, ChaindexingRepoRawQueryTxnClient, ExecutesWithRawQuery, LoadsDataWithRawQuery,
};

use super::state_versions::{StateVersion, StateVersions, STATE_VERSIONS_UNIQUE_FIELDS};
use super::{serde_map_to_string_map, to_and_filters, to_columns_and_values};

pub struct StateViews;

impl StateViews {
    pub async fn refresh<'a>(
        state_version_group_ids: &Vec<String>,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let backtracked_state_versions =
            StateVersions::get_latest(&state_version_group_ids, table_name, client).await;

        for latest_state_version in backtracked_state_versions {
            StateView::refresh(&latest_state_version, table_name, client).await
        }
    }
}
pub struct StateView;

impl StateView {
    pub async fn get_complete<'a>(
        state_view: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let query = format!(
            "SELECT * FROM {table_name} WHERE {filters}",
            filters = to_and_filters(state_view),
        );

        serde_map_to_string_map(
            ChaindexingRepo::load_data_from_raw_query_with_txn_client::<
                HashMap<String, serde_json::Value>,
            >(client, &query)
            .await
            .unwrap(),
        )
    }

    pub async fn refresh<'a>(
        latest_state_version: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let state_version_group_id = StateVersion::get_group_id(latest_state_version);

        if StateVersion::was_deleted(&latest_state_version) {
            Self::delete(&state_version_group_id, table_name, client).await;
        } else {
            let new_state_view = Self::from_latest_state_version(&latest_state_version);

            Self::delete(&state_version_group_id, table_name, client).await;
            Self::create(&new_state_view, table_name, client).await;
        }
    }

    fn from_latest_state_version(
        latest_state_version: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        latest_state_version
            .clone()
            .into_iter()
            .filter(|(field, _value)| !STATE_VERSIONS_UNIQUE_FIELDS.contains(&field.as_str()))
            .collect()
    }

    async fn delete<'a>(
        state_version_group_id: &str,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let query = format!(
            "DELETE FROM {table_name} WHERE state_version_group_id = '{state_version_group_id}'",
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }

    async fn create<'a>(
        new_state_view: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let (columns, values) = to_columns_and_values(&new_state_view);
        let query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            columns = columns.join(","),
            values = values.join(",")
        );

        ChaindexingRepo::execute_raw_query_in_txn(client, &query).await;
    }
}
