use std::collections::HashMap;

use crate::handlers::{HandlerContext, PureHandlerContext};
use crate::{ChaindexingRepo, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo};

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
    let client = context.get_client();

    let query = format!(
        "SELECT * FROM {table_name} 
        WHERE {filters}",
        table_name = table_name,
        filters = to_and_filters(&filters.get(context.get_event())),
    );

    ChaindexingRepo::load_data_list_in_txn(client, &query).await
}

pub async fn read_many_from_postgres<T: Send + DeserializeOwned>(
    postgres_url: &str,
    table_name: &str,
    filters: HashMap<String, String>,
) -> Vec<T> {
    let repo = PostgresRepo::new(postgres_url);
    let client = repo.get_client().await;

    ChaindexingRepo::load_data_list(&client, &read_query(table_name, &filters)).await
}

pub async fn create<'a, 'b>(
    table_name: &str,
    state_view: &HashMap<String, String>,
    context: &PureHandlerContext<'a, 'b>,
) {
    let event = &context.event;
    let client = context.repo_client;

    let latest_state_version = StateVersion::create(state_view, table_name, event, client).await;
    StateView::refresh(&latest_state_version, table_name, client).await;
}

fn read_query(table_name: &str, filters: &HashMap<String, String>) -> String {
    if filters.is_empty() {
        format!("SELECT * FROM {table_name}")
    } else {
        format!(
            "SELECT * FROM {table_name}
            WHERE {filters}",
            filters = to_and_filters(filters)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_query_escapes_filter_values() {
        let filters = HashMap::from([("owner".to_string(), "owner's wallet".to_string())]);

        let query = read_query("nfts", &filters);

        assert!(query.contains("owner = 'owner''s wallet'"));
    }

    #[test]
    fn read_query_allows_empty_filters() {
        assert_eq!(read_query("nfts", &HashMap::new()), "SELECT * FROM nfts");
    }
}
