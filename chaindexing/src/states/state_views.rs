use std::collections::HashMap;

use crate::{ChaindexingRepo, ChaindexingRepoTxnClient};
use crate::{ChaindexingRepoClient, Event};
use crate::{ExecutesWithRawQuery, LoadsDataWithRawQuery};

use super::state_versions::{StateVersion, StateVersions, STATE_VERSIONS_UNIQUE_FIELDS};
use super::{serde_map_to_string_map, to_and_filters, to_columns_and_values};

pub struct StateViews;

impl StateViews {
    pub async fn refresh<'a>(
        state_version_group_ids: &[String],
        table_name: &str,
        client: &ChaindexingRepoTxnClient<'a>,
    ) {
        let latest_state_versions =
            StateVersions::get_latest(state_version_group_ids, table_name, client).await;

        for latest_state_version in latest_state_versions {
            StateView::refresh(&latest_state_version, table_name, client).await
        }
    }
}

pub struct StateView;

impl StateView {
    pub async fn get_complete<'a>(
        state_view: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoTxnClient<'a>,
        event: &Event,
    ) -> HashMap<String, String> {
        let context_chain_id = event.chain_id;
        let context_contract_address = &event.contract_address;

        let query = format!(
            "SELECT * FROM {table_name} WHERE {filters} 
            AND chain_id={context_chain_id} AND contract_address='{context_contract_address}'",
            filters = to_and_filters(state_view),
        );

        serde_map_to_string_map(
            &ChaindexingRepo::load_data_in_txn::<HashMap<String, serde_json::Value>>(
                client, &query,
            )
            .await
            .unwrap(),
        )
    }

    pub async fn refresh<'a>(
        latest_state_version: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoTxnClient<'a>,
    ) {
        let state_version_group_id = StateVersion::get_group_id(latest_state_version);

        if StateVersion::was_deleted(latest_state_version) {
            Self::delete(&state_version_group_id, table_name, client).await;
        } else {
            let new_state_view = Self::from_latest_state_version(latest_state_version);

            Self::delete(&state_version_group_id, table_name, client).await;
            Self::create(&new_state_view, table_name, client).await;
        }
    }

    pub async fn refresh_without_txn(
        latest_state_version: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoClient,
    ) {
        let state_version_group_id = StateVersion::get_group_id(latest_state_version);

        if StateVersion::was_deleted(latest_state_version) {
            Self::delete_without_txn(&state_version_group_id, table_name, client).await;
        } else {
            let new_state_view = Self::from_latest_state_version(latest_state_version);

            Self::delete_without_txn(&state_version_group_id, table_name, client).await;
            Self::create_without_txn(&new_state_view, table_name, client).await;
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
        client: &ChaindexingRepoTxnClient<'a>,
    ) {
        ChaindexingRepo::execute_in_txn(
            client,
            &Self::delete_query(state_version_group_id, table_name),
        )
        .await;
    }
    async fn delete_without_txn(
        state_version_group_id: &str,
        table_name: &str,
        client: &ChaindexingRepoClient,
    ) {
        ChaindexingRepo::execute(
            client,
            &Self::delete_query(state_version_group_id, table_name),
        )
        .await;
    }
    fn delete_query(state_version_group_id: &str, table_name: &str) -> String {
        format!(
            "DELETE FROM {table_name} WHERE state_version_group_id = '{state_version_group_id}'",
        )
    }

    async fn create<'a>(
        new_state_view: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoTxnClient<'a>,
    ) {
        ChaindexingRepo::execute_in_txn(client, &Self::create_query(new_state_view, table_name))
            .await;
    }
    async fn create_without_txn(
        new_state_view: &HashMap<String, String>,
        table_name: &str,
        client: &ChaindexingRepoClient,
    ) {
        ChaindexingRepo::execute(client, &Self::create_query(new_state_view, table_name)).await;
    }
    fn create_query(new_state_view: &HashMap<String, String>, table_name: &str) -> String {
        let (columns, values) = to_columns_and_values(new_state_view);
        format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            columns = columns.join(","),
            values = values.join(",")
        )
    }
}
