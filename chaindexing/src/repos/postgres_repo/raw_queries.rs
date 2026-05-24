use tokio_postgres::{types::ToSql, Client, Transaction};

use crate::chain_reorg::ReorgedBlock;
use crate::checkpoints::{self, CheckpointKind};
use crate::events::PartialEvent;
use crate::nodes::Node;
use crate::{root, Event, UnsavedContractAddress};
use crate::{ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo};

use super::tls;
use serde::de::DeserializeOwned;

pub type PostgresRepoClient = Client;
pub type PostgresRepoTxnClient<'a> = Transaction<'a>;

#[crate::augmenting_std::async_trait]
impl HasRawQueryClient for PostgresRepo {
    type RawQueryClient = Client;
    type RawQueryTxnClient<'a> = Transaction<'a>;

    async fn get_client(&self) -> Self::RawQueryClient {
        tls::connect_raw_client(&self.url, self.tls_config()).await.unwrap()
    }
    async fn get_txn_client<'a>(
        client: &'a mut Self::RawQueryClient,
    ) -> Self::RawQueryTxnClient<'a> {
        client.transaction().await.unwrap()
    }
}

#[crate::augmenting_std::async_trait]
impl ExecutesWithRawQuery for PostgresRepo {
    async fn execute(client: &Self::RawQueryClient, query: &str) {
        client.execute(query, &[] as &[&(dyn ToSql + Sync)]).await.unwrap();
    }
    async fn execute_in_txn<'a>(txn_client: &Self::RawQueryTxnClient<'a>, query: &str) {
        txn_client.execute(query, &[] as &[&(dyn ToSql + Sync)]).await.unwrap();
    }
    async fn commit_txns<'a>(client: Self::RawQueryTxnClient<'a>) {
        client.commit().await.unwrap();
    }

    async fn create_contract_addresses(
        client: &Self::RawQueryClient,
        contract_addresses: &[UnsavedContractAddress],
    ) {
        if contract_addresses.is_empty() {
            return;
        }

        let contract_addresses_values = contract_address_values(contract_addresses);

        let query = format!("
            INSERT INTO chaindexing_contract_addresses 
            (address, chain_id, contract_name, next_block_number_to_handle_from, next_block_number_to_ingest_from, start_block_number)
            VALUES {contract_addresses_values}
            ON CONFLICT (chain_id, address)
            DO NOTHING
        ");

        Self::execute(client, &query).await;
    }

    async fn create_contract_address<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        contract_address: &UnsavedContractAddress,
    ) {
        let address = sql_string_literal(&contract_address.address);
        let contract_name = sql_string_literal(&contract_address.contract_name);
        let chain_id = contract_address.chain_id;
        let start_block_number = contract_address.start_block_number;

        let query = format!(
            "INSERT INTO chaindexing_contract_addresses 
            (address, chain_id, contract_name, next_block_number_to_handle_from, next_block_number_to_ingest_from, start_block_number)
            VALUES ({address}, {chain_id}, {contract_name}, {start_block_number}, {start_block_number}, {start_block_number})
            ON CONFLICT (chain_id, address)
            DO NOTHING"
        );

        Self::execute_in_txn(client, &query).await;
    }

    async fn update_next_block_number_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        address: &str,
        chain_id: u64,
        block_number: u64,
    ) {
        let query = format!(
            "UPDATE chaindexing_contract_addresses
        SET next_block_number_to_handle_from = {block_number}
        WHERE chain_id = {chain_id} AND address = {address}",
            address = sql_string_literal(address),
        );

        Self::execute_in_txn(client, &query).await;
        Self::execute_in_txn(
            client,
            &checkpoints::upsert_query(chain_id, address, CheckpointKind::Reducer, block_number),
        )
        .await;
    }

    async fn update_next_block_numbers_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_id: u64,
        block_number: u64,
    ) {
        let query = format!(
            "UPDATE chaindexing_contract_addresses
        SET next_block_number_to_handle_from = {block_number}
        WHERE chain_id = {chain_id}"
        );

        Self::execute_in_txn(client, &query).await;
        Self::execute_in_txn(
            client,
            &checkpoints::upsert_all_for_chain_query(
                chain_id,
                CheckpointKind::Reducer,
                block_number,
            ),
        )
        .await;
    }

    async fn update_next_block_number_for_side_effects<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        address: &str,
        chain_id: u64,
        block_number: u64,
    ) {
        let query = format!(
            "UPDATE chaindexing_contract_addresses
        SET next_block_number_for_side_effects = {block_number}
        WHERE chain_id = {chain_id} AND address = {address}",
            address = sql_string_literal(address),
        );

        Self::execute_in_txn(client, &query).await;
        Self::execute_in_txn(
            client,
            &checkpoints::upsert_query(chain_id, address, CheckpointKind::SideEffect, block_number),
        )
        .await;
    }

    async fn update_reorged_blocks_as_handled<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        reorged_block_ids: &[i32],
    ) {
        // Skip the query if there are no IDs to update
        if reorged_block_ids.is_empty() {
            return;
        }

        let query = format!(
            "UPDATE chaindexing_reorged_blocks
            SET handled_at = '{handled_at}'
            WHERE id IN ({reorged_block_ids})",
            reorged_block_ids = join_numbers_with_comma(reorged_block_ids),
            handled_at = chrono::Utc::now().timestamp(),
        );

        Self::execute_in_txn(client, &query).await;
    }

    async fn append_root_state(client: &Self::RawQueryClient, new_root_state: &root::State) {
        let reset_count = new_root_state.reset_count;
        let reset_including_side_effects_count = new_root_state.reset_including_side_effects_count;

        let query = format!(
            "INSERT INTO chaindexing_root_states
            (reset_count, reset_including_side_effects_count)
            VALUES ('{reset_count}', '{reset_including_side_effects_count}')"
        );

        Self::execute(client, &query).await;
    }

    async fn prune_events(client: &Self::RawQueryClient, min_block_number: u64, chain_id: u64) {
        let query = format!(
            "DELETE FROM chaindexing_events
            WHERE block_number < {min_block_number}
            AND chain_id = {chain_id}
            "
        );

        Self::execute(client, &query).await;
    }

    async fn prune_nodes(client: &Self::RawQueryClient, retain_size: u16) {
        let query = format!(
            "DELETE FROM chaindexing_nodes
            WHERE id NOT IN (
                SELECT id
                FROM chaindexing_nodes
                ORDER BY id DESC
                LIMIT {retain_size}
            )
            "
        );

        Self::execute(client, &query).await;
    }

    async fn prune_root_states(client: &Self::RawQueryClient, retain_size: u64) {
        let query = format!(
            "DELETE FROM chaindexing_root_states
            WHERE id NOT IN (
                SELECT id
                FROM chaindexing_root_states
                ORDER BY id DESC
                LIMIT {retain_size}
            )
            "
        );

        Self::execute(client, &query).await;
    }
}

#[crate::augmenting_std::async_trait]
impl LoadsDataWithRawQuery for PostgresRepo {
    async fn create_and_load_new_node(client: &Self::RawQueryClient) -> Node {
        Self::load_data(
            client,
            "INSERT INTO chaindexing_nodes DEFAULT VALUES RETURNING *",
        )
        .await
        .unwrap()
    }

    async fn load_last_root_state(client: &Self::RawQueryClient) -> Option<root::State> {
        Self::load_data(
            client,
            "SELECT * FROM chaindexing_root_states
            ORDER BY id DESC
            LIMIT 1",
        )
        .await
    }

    async fn load_events(
        client: &Self::RawQueryClient,
        chain_id: u64,
        contract_address: &str,
        from_block_number: u64,
        block_limit: u64,
    ) -> Vec<Event> {
        Self::load_data_list(
            client,
            &load_events_query(chain_id, contract_address, from_block_number, block_limit),
        )
        .await
    }

    async fn load_latest_events(
        client: &Self::RawQueryClient,
        chain_id: u64,
        addresses: &[String],
    ) -> Vec<PartialEvent> {
        if addresses.is_empty() {
            return vec![];
        }

        let query = format!(
            "WITH EventsWithRowNumbers AS (
                SELECT
                    *,
                    ROW_NUMBER() OVER (PARTITION BY contract_address ORDER BY block_number DESC, log_index DESC) AS row_no
                FROM
                    chaindexing_events
                WHERE
                chain_id = {chain_id}
                AND
                contract_address IN ({addresses})
                AND status = 'canonical'
            )
            SELECT
                *
            FROM
                EventsWithRowNumbers
            WHERE
                row_no = 1",
                addresses = join_strings_with_comma(addresses),
        );

        Self::load_data_list(client, &query).await
    }
    async fn load_unhandled_reorged_blocks(client: &Self::RawQueryClient) -> Vec<ReorgedBlock> {
        Self::load_data_list(
            client,
            "SELECT * FROM chaindexing_reorged_blocks WHERE handled_at is NULL",
        )
        .await
    }

    async fn load_data<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Option<Data> {
        let mut data_list: Vec<Data> = Self::load_data_list(client, query).await;

        assert!(data_list.len() <= 1);

        data_list.pop()
    }
    async fn load_data_in_txn<'a, Data: Send + DeserializeOwned>(
        client: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Option<Data> {
        let mut data_list: Vec<Data> = Self::load_data_list_in_txn(client, query).await;

        assert!(data_list.len() <= 1);

        data_list.pop()
    }

    async fn load_data_list<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Vec<Data> {
        let json_aggregate = get_json_aggregate(client, query).await;

        if json_aggregate.is_object() || json_aggregate.is_array() {
            serde_json::from_value(json_aggregate).unwrap()
        } else {
            vec![]
        }
    }

    async fn load_data_list_in_txn<'a, Data: Send + DeserializeOwned>(
        txn_client: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Vec<Data> {
        let json_aggregate = get_json_aggregate_in_txn(txn_client, query).await;

        if json_aggregate.is_object() || json_aggregate.is_array() {
            serde_json::from_value(json_aggregate).unwrap()
        } else {
            vec![]
        }
    }
}

async fn get_json_aggregate(client: &PostgresRepoClient, query: &str) -> serde_json::Value {
    let rows = client.query(json_aggregate_query(query).as_str(), &[]).await.unwrap();
    rows.first().unwrap().get(0)
}

async fn get_json_aggregate_in_txn<'a>(
    txn_client: &PostgresRepoTxnClient<'a>,
    query: &str,
) -> serde_json::Value {
    let rows = txn_client.query(json_aggregate_query(query).as_str(), &[]).await.unwrap();
    rows.first().unwrap().get(0)
}

fn json_aggregate_query(query: &str) -> String {
    format!("WITH result AS ({query}) SELECT COALESCE(json_agg(result), '[]'::json) FROM result",)
}

fn load_events_query(
    chain_id: u64,
    contract_address: &str,
    from_block_number: u64,
    block_limit: u64,
) -> String {
    format!(
        "WITH event_blocks AS (
            SELECT DISTINCT block_number
            FROM chaindexing_events
            WHERE chain_id = {chain_id}
              AND contract_address = {contract_address}
              AND block_number >= {from_block_number}
              AND status = 'canonical'
            ORDER BY block_number ASC
            LIMIT {block_limit}
        )
        SELECT e.*
        FROM chaindexing_events e
        INNER JOIN event_blocks b
            ON b.block_number = e.block_number
        WHERE e.chain_id = {chain_id}
          AND e.contract_address = {contract_address}
          AND e.status = 'canonical'
        ORDER BY e.block_number ASC, e.transaction_index ASC, e.log_index ASC",
        contract_address = sql_string_literal(contract_address),
    )
}

fn join_numbers_with_comma(numbers: &[impl ToString]) -> String {
    numbers.iter().map(|n| n.to_string()).collect::<Vec<String>>().join(",")
}

fn join_strings_with_comma(strings: &[String]) -> String {
    strings
        .iter()
        .map(|string| sql_string_literal(string))
        .collect::<Vec<_>>()
        .join(",")
}

fn contract_address_values(contract_addresses: &[UnsavedContractAddress]) -> String {
    contract_addresses
        .iter()
        .map(
            |UnsavedContractAddress {
                 address,
                 chain_id,
                 contract_name,
                 start_block_number,
                 ..
             }| {
                format!(
                    "({}, {chain_id}, {}, {start_block_number}, {start_block_number}, {start_block_number})",
                    sql_string_literal(address),
                    sql_string_literal(contract_name),
                )
            },
        )
        .collect::<Vec<_>>()
        .join(",")
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChainId;

    #[test]
    fn joins_strings_as_escaped_sql_literals() {
        let values = vec!["owner's wallet".to_string(), "plain".to_string()];

        assert_eq!(
            join_strings_with_comma(&values),
            "'owner''s wallet','plain'"
        );
    }

    #[test]
    fn load_events_query_limits_blocks_not_events() {
        let query = load_events_query(1, "0xabc'def", 100, 10);

        assert!(query.contains("SELECT DISTINCT block_number"));
        assert!(query.contains("LIMIT 10"));
        assert!(query.contains("contract_address = '0xabc''def'"));
        assert!(!query.contains("SELECT e.*\n        FROM chaindexing_events e\n        WHERE"));
    }

    #[test]
    fn contract_address_values_escapes_user_strings() {
        let values = contract_address_values(&[UnsavedContractAddress::new(
            "ERC'721",
            "0xabc'def",
            &ChainId::Mainnet,
            10,
        )]);

        assert!(values.contains("'ERC''721'"));
        assert!(values.contains("'0xabc''def'"));
    }
}
