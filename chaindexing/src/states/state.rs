use std::collections::HashMap;

use crate::handlers::{HandlerContext, PureHandlerContext};
use crate::{ChaindexingRepo, LoadsDataWithRawQuery};

use super::filters::Filters;
use super::state_versions::StateVersion;
use super::state_views::StateView;
use super::{serde_map_to_string_map, to_and_filters};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub fn to_view<T>(value: &T) -> HashMap<String, String>
where
    T: Serialize,
{
    let state: serde_json::Value = serde_json::to_value(value).unwrap();

    let map: HashMap<String, serde_json::Value> = serde_json::from_value(state).unwrap();

    serde_map_to_string_map(&map)
}

pub async fn read_many<'a, C: HandlerContext<'a>, T: Send + DeserializeOwned>(
    filters: &Filters,
    context: &C,
    table_name: &str,
) -> Vec<T> {
    let client = context.get_raw_query_client();

    let raw_query = format!(
        "SELECT * FROM {table_name} 
        WHERE {filters}",
        table_name = table_name,
        filters = to_and_filters(&filters.get(context.get_event())),
    );

    ChaindexingRepo::load_data_list_from_raw_query_with_txn_client(client, &raw_query).await
}

pub async fn create<'a, 'b>(
    table_name: &str,
    state_view: &HashMap<String, String>,
    context: &PureHandlerContext<'a, 'b>,
) {
    let event = &context.event;
    let client = context.raw_query_client;

    let latest_state_version = StateVersion::create(state_view, table_name, event, client).await;
    StateView::refresh(&latest_state_version, table_name, client).await;
}
