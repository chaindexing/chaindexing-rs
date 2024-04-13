use tokio_postgres::{types::ToSql, Client, NoTls, Transaction};

use crate::chain_reorg::ReorgedBlock;
use crate::event_handler_subscriptions::EventHandlerSubscription;
use crate::events::PartialEvent;
use crate::{Event, UnsavedContractAddress};
use crate::{ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo};
use serde::de::DeserializeOwned;

pub type PostgresRepoRawQueryClient = Client;
pub type PostgresRepoRawQueryTxnClient<'a> = Transaction<'a>;

#[async_trait::async_trait]
impl HasRawQueryClient for PostgresRepo {
    type RawQueryClient = Client;
    type RawQueryTxnClient<'a> = Transaction<'a>;

    async fn get_raw_query_client(&self) -> Self::RawQueryClient {
        let (client, conn) = tokio_postgres::connect(&self.url, NoTls).await.unwrap();

        tokio::spawn(async move { conn.await.map_err(|e| eprintln!("connection error: {}", e)) });

        client
    }
    async fn get_raw_query_txn_client<'a>(
        client: &'a mut Self::RawQueryClient,
    ) -> Self::RawQueryTxnClient<'a> {
        client.transaction().await.unwrap()
    }
}

#[async_trait::async_trait]
impl ExecutesWithRawQuery for PostgresRepo {
    async fn execute_raw_query(client: &Self::RawQueryClient, query: &str) {
        client.execute(query, &[] as &[&(dyn ToSql + Sync)]).await.unwrap();
    }
    async fn execute_raw_query_in_txn<'a>(txn_client: &Self::RawQueryTxnClient<'a>, query: &str) {
        txn_client.execute(query, &[] as &[&(dyn ToSql + Sync)]).await.unwrap();
    }
    async fn commit_raw_query_txns<'a>(client: Self::RawQueryTxnClient<'a>) {
        client.commit().await.unwrap();
    }

    async fn create_event_handler_subscriptions(
        client: &Self::RawQueryClient,
        event_handler_subscriptions: &[EventHandlerSubscription],
    ) {
        for EventHandlerSubscription {
            chain_id,
            next_block_number_to_handle_from,
        } in event_handler_subscriptions
        {
            let query = format!(
                "INSERT INTO chaindexing_event_handler_subscriptions 
                (chain_id, next_block_number_to_handle_from)
                VALUES ('{chain_id}', {next_block_number_to_handle_from})"
            );

            Self::execute_raw_query(client, &query).await;
        }
    }

    async fn create_contract_address<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        contract_address: &UnsavedContractAddress,
    ) {
        let address = &contract_address.address;
        let contract_name = &contract_address.contract_name;
        let chain_id = contract_address.chain_id;
        let start_block_number = contract_address.start_block_number;

        let query = format!(
            "INSERT INTO chaindexing_contract_addresses 
            (address, contract_name, chain_id, start_block_number, next_block_number_to_ingest_from)
            VALUES ('{address}', '{contract_name}', {chain_id}, {start_block_number}, {start_block_number})"
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn update_event_handler_subscriptions_next_block<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_ids: &[u64],
        next_block_number_to_handle_from: u64,
    ) {
        let query = format!(
            "UPDATE chaindexing_event_handler_subscriptions
        SET next_block_number_to_handle_from = {next_block_number_to_handle_from}
        WHERE chain_id IN ({chain_ids})",
            chain_ids = join_numbers_with_comma(chain_ids),
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn update_every_next_block_number_to_handle_from<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        chain_id: i64,
        block_number: i64,
    ) {
        let query = format!(
            "UPDATE chaindexing_contract_addresses 
        SET next_block_number_to_handle_from = {block_number}
        WHERE chain_id = {chain_id}"
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn update_reorged_blocks_as_handled<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        reorged_block_ids: &[i32],
    ) {
        let query = format!(
            "UPDATE chaindexing_reorged_blocks
        SET handled_at = '{handled_at}'
        WHERE id IN ({reorged_block_ids})",
            reorged_block_ids = join_numbers_with_comma(reorged_block_ids),
            handled_at = chrono::Utc::now().naive_utc(),
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn prune_events(client: &Self::RawQueryClient, min_block_number: u64, chain_id: u64) {
        let query = format!(
            "
            DELETE FROM chaindexing_events
            WHERE block_number < {min_block_number}
            AND chain_id = {chain_id}
            "
        );

        Self::execute_raw_query(client, &query).await;
    }

    async fn prune_nodes(client: &Self::RawQueryClient, retain_size: u16) {
        let query = format!(
            "
            DELETE FROM chaindexing_nodes
            WHERE id NOT IN (
                SELECT id
                FROM chaindexing_nodes
                ORDER BY id DESC
                LIMIT {retain_size}
            )
            "
        );

        Self::execute_raw_query(client, &query).await;
    }

    async fn prune_reset_counts(client: &Self::RawQueryClient, retain_size: u64) {
        let query = format!(
            "
            DELETE FROM chaindexing_reset_counts
            WHERE id NOT IN (
                SELECT id
                FROM chaindexing_reset_counts
                ORDER BY id DESC
                LIMIT {retain_size}
            )
            "
        );

        Self::execute_raw_query(client, &query).await;
    }
}

#[async_trait::async_trait]
impl LoadsDataWithRawQuery for PostgresRepo {
    async fn load_event_handler_subscriptions(
        client: &Self::RawQueryClient,
        chain_ids: &[u64],
    ) -> Vec<EventHandlerSubscription> {
        let query = format!(
            "SELECT * from chaindexing_event_handler_subscriptions
            WHERE chain_id IN ({chain_ids})",
            chain_ids = join_numbers_with_comma(chain_ids),
        );

        Self::load_data_list_from_raw_query(client, &query).await
    }

    async fn load_events(
        client: &Self::RawQueryClient,
        chain_ids: &[u64],
        from_block_number: u64,
        to_block_number: u64,
    ) -> Vec<Event> {
        let query = format!(
            "SELECT * from chaindexing_events
            WHERE chain_id IN ({chain_ids})
            AND block_number BETWEEN {from_block_number} AND {to_block_number} 
             ",
            chain_ids = join_numbers_with_comma(chain_ids),
        );

        Self::load_data_list_from_raw_query(client, &query).await
    }

    async fn load_latest_events(
        client: &Self::RawQueryClient,
        addresses: &[String],
    ) -> Vec<PartialEvent> {
        let query = format!(
            "WITH EventsWithRowNumbers AS (
                SELECT
                    *,
                    ROW_NUMBER() OVER (PARTITION BY contract_address ORDER BY block_number DESC, log_index DESC) AS row_no
                FROM
                    chaindexing_events
                WHERE
                contract_address IN ({addresses})
            )
            SELECT
                *
            FROM
                EventsWithRowNumbers
            WHERE
                row_no = 1",
                addresses = join_strings_with_comma(addresses),
        );

        Self::load_data_list_from_raw_query(client, &query).await
    }
    async fn load_unhandled_reorged_blocks(client: &Self::RawQueryClient) -> Vec<ReorgedBlock> {
        Self::load_data_list_from_raw_query(
            client,
            "SELECT * FROM chaindexing_reorged_blocks WHERE handled_at is NULL",
        )
        .await
    }

    async fn load_data_from_raw_query<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Option<Data> {
        let mut data_list: Vec<Data> = Self::load_data_list_from_raw_query(client, query).await;

        assert!(data_list.len() <= 1);

        data_list.pop()
    }
    async fn load_data_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
        client: &Self::RawQueryTxnClient<'a>,
        query: &str,
    ) -> Option<Data> {
        let mut data_list: Vec<Data> =
            Self::load_data_list_from_raw_query_with_txn_client(client, query).await;

        assert!(data_list.len() <= 1);

        data_list.pop()
    }

    async fn load_data_list_from_raw_query<Data: Send + DeserializeOwned>(
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

    async fn load_data_list_from_raw_query_with_txn_client<'a, Data: Send + DeserializeOwned>(
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

async fn get_json_aggregate(client: &PostgresRepoRawQueryClient, query: &str) -> serde_json::Value {
    let rows = client.query(json_aggregate_query(query).as_str(), &[]).await.unwrap();
    rows.first().unwrap().get(0)
}

async fn get_json_aggregate_in_txn<'a>(
    txn_client: &PostgresRepoRawQueryTxnClient<'a>,
    query: &str,
) -> serde_json::Value {
    let rows = txn_client.query(json_aggregate_query(query).as_str(), &[]).await.unwrap();
    rows.first().unwrap().get(0)
}

fn json_aggregate_query(query: &str) -> String {
    format!("WITH result AS ({query}) SELECT COALESCE(json_agg(result), '[]'::json) FROM result",)
}

fn join_numbers_with_comma(numbers: &[impl ToString]) -> String {
    numbers.iter().map(|n| n.to_string()).collect::<Vec<String>>().join(",")
}

fn join_strings_with_comma(strings: &[String]) -> String {
    strings.iter().map(|string| format!("'{string}'")).collect::<Vec<_>>().join(",")
}
