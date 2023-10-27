use std::sync::Arc;
use std::{collections::HashMap, fmt::Debug};

mod migrations;
mod state_versions;
mod state_views;

pub use crate::event_handlers::EventHandlerContext;
use crate::{ChaindexingRepo, ChaindexingRepoRawQueryTxnClient, LoadsDataWithRawQuery};
pub use migrations::ContractStateMigrations;

use serde::de::DeserializeOwned;
use serde::Serialize;
use state_versions::{StateVersion, StateVersions, STATE_VERSIONS_TABLE_PREFIX};
use state_views::{StateView, StateViews};

pub struct ContractStates;

impl ContractStates {
    pub async fn backtrack_states<'a>(
        state_migrations: &Vec<Arc<dyn ContractStateMigrations>>,
        chain_id: i32,
        block_number: i64,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) {
        let table_names = Self::get_all_table_names(&state_migrations);

        for table_name in table_names {
            let state_versions =
                StateVersions::get(block_number, chain_id, &table_name, client).await;

            let state_version_ids = StateVersions::get_ids(&state_versions);
            StateVersions::delete_by_ids(&state_version_ids, &table_name, client).await;

            let state_version_group_ids = StateVersions::get_group_ids(&state_versions);
            StateViews::refresh(&state_version_group_ids, &table_name, client).await;
        }
    }

    pub fn get_all_table_names(
        state_migrations: &Vec<Arc<dyn ContractStateMigrations>>,
    ) -> Vec<String> {
        state_migrations
            .iter()
            .flat_map(|state_migration| state_migration.get_table_names())
            .collect()
    }
}

// TODO:
// Investigate HashMap Interface Vs Json (Serde)
// Move Queries to Repo level and prevent SQLInjection
// Create Into<StateVersionEvent> to extract events fields we care about once and avoid passing around the whole Event struct
#[async_trait::async_trait]
pub trait ContractState:
    DeserializeOwned + Serialize + Clone + Debug + Sync + Send + 'static
{
    fn table_name() -> &'static str;

    fn to_view(&self) -> HashMap<String, String> {
        let state: serde_json::Value = serde_json::to_value(self).unwrap();

        let map: HashMap<String, serde_json::Value> = serde_json::from_value(state).unwrap();

        serde_map_to_string_map(map)
    }

    async fn to_complete_view<'a>(
        &self,
        table_name: &str,
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> HashMap<String, String> {
        let view = self.to_view();

        StateView::get_complete(&view, table_name, client).await
    }

    async fn create<'a>(&self, context: &EventHandlerContext) {
        let event = &context.event;
        let client = context.get_raw_query_client();

        let state_view = self.to_view();
        let table_name = Self::table_name();

        let latest_state_version =
            StateVersion::create(&state_view, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    async fn update<'a>(&self, updates: HashMap<String, String>, context: &EventHandlerContext) {
        let event = &context.event;
        let client = context.get_raw_query_client();

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(&table_name, &client).await;

        let latest_state_version =
            StateVersion::update(&state_view, &updates, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    async fn delete<'a>(&self, context: &EventHandlerContext) {
        let event = &context.event;
        let client = context.get_raw_query_client();

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(&table_name, &client).await;

        let latest_state_version =
            StateVersion::delete(&state_view, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    async fn read_one<'a>(
        filters: HashMap<String, String>,
        context: &EventHandlerContext,
    ) -> Option<Self> {
        let states = Self::read_many(filters, context).await;

        states.first().cloned()
    }

    async fn read_many<'a>(
        filters: HashMap<String, String>,
        context: &EventHandlerContext,
    ) -> Vec<Self> {
        let client = context.get_raw_query_client();

        let raw_query = format!(
            "SELECT * FROM {table_name} WHERE {filters}",
            table_name = Self::table_name(),
            filters = to_and_filters(&filters),
        );

        ChaindexingRepo::load_data_list_from_raw_query_with_txn_client(client, &raw_query).await
    }

    async fn get_state_fields<'a>(
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
    ) -> Option<Vec<String>> {
        Self::get_random_state(client)
            .await
            .and_then(|random_state| Some(random_state.get_fields()))
    }
    fn get_fields(&self) -> Vec<String> {
        self.to_view().keys().cloned().collect()
    }
    async fn get_random_state<'a>(client: &ChaindexingRepoRawQueryTxnClient<'a>) -> Option<Self> {
        let table_name = Self::table_name();
        let query = format!("SELECT * from {table_name} limit 1");

        ChaindexingRepo::load_data_from_raw_query_with_txn_client(client, &query).await
    }
}

pub fn to_columns_and_values(state: &HashMap<String, String>) -> (Vec<String>, Vec<String>) {
    state.into_iter().fold(
        (vec![], vec![]),
        |(mut columns, mut values), (column, value)| {
            columns.push(column.to_string());
            values.push(format!("'{value}'"));

            (columns, values)
        },
    )
}

pub fn to_and_filters(state: &HashMap<String, String>) -> String {
    let filters = state.iter().fold(vec![], |mut filters, (column, value)| {
        filters.push(format!("{column} = '{value}'"));

        filters
    });

    filters.join(" AND ")
}

pub fn serde_map_to_string_map(
    serde_map: HashMap<String, serde_json::Value>,
) -> HashMap<String, String> {
    serde_map.iter().fold(HashMap::new(), |mut map, (key, value)| {
        map.insert(key.to_owned(), value.to_string().replace("\"", ""));

        map
    })
}
