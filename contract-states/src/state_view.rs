use crate::state_version;

pub async fn get_complete<'a>(
    state_view: &HashMap<String, String>,
    table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
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
        Repo::load_data_from_raw_query_with_txn_client::<HashMap<String, serde_json::Value>>(
            client, &query,
        )
        .await
        .unwrap(),
    )
}

pub async fn refresh<'a>(
    latest_state_version: &HashMap<String, String>,
    table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) {
    let state_version_group_id = state_version::get_group_id(latest_state_version);

    if state_version::was_deleted(latest_state_version) {
        delete(&state_version_group_id, table_name, client).await;
    } else {
        let new_state_view = from_latest_state_version(latest_state_version);

        delete(&state_version_group_id, table_name, client).await;
        create(&new_state_view, table_name, client).await;
    }
}

fn from_latest_state_version(
    latest_state_version: &HashMap<String, String>,
) -> HashMap<String, String> {
    latest_state_version
        .clone()
        .into_iter()
        .filter(|(field, _value)| !state_versions::UNIQUE_FIELDS.contains(&field.as_str()))
        .collect()
}

async fn delete<'a>(
    state_version_group_id: &str,
    table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) {
    let query = format!(
        "DELETE FROM {table_name} WHERE state_version_group_id = '{state_version_group_id}'",
    );

    Repo::execute_raw_query_in_txn(client, &query).await;
}

async fn create<'a>(
    new_state_view: &HashMap<String, String>,
    table_name: &str,
    client: &RepoRawQueryTxnClient<'a>,
) {
    let (columns, values) = to_columns_and_values(new_state_view);
    let query = format!(
        "INSERT INTO {table_name} ({columns}) VALUES ({values})",
        columns = columns.join(","),
        values = values.join(",")
    );

    Repo::execute_raw_query_in_txn(client, &query).await;
}
