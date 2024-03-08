use tokio_postgres::{types::ToSql, Client, NoTls, Transaction};

use crate::contracts::ContractAddressID;
use crate::events::PartialEvent;
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

    async fn update_next_block_number_to_handle_from_in_txn<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        ContractAddressID(contract_address_id): ContractAddressID,
        block_number: i64,
    ) {
        let query = format!(
            "UPDATE chaindexing_contract_addresses 
        SET next_block_number_to_handle_from = {block_number}
        WHERE id = {contract_address_id}"
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn update_every_next_block_number_to_handle_from_in_txn<'a>(
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

    async fn update_reorged_blocks_as_handled_in_txn<'a>(
        client: &Self::RawQueryTxnClient<'a>,
        reorged_block_ids: &[i32],
    ) {
        let query = format!(
            "UPDATE chaindexing_reorged_blocks
        SET handled_at = '{handled_at}'
        WHERE id IN ({reorged_block_ids})",
            reorged_block_ids = join_i32s_with_comma(reorged_block_ids),
            handled_at = chrono::Utc::now().naive_utc(),
        );

        Self::execute_raw_query_in_txn(client, &query).await;
    }

    async fn prune_nodes(client: &Self::RawQueryClient, prune_size: u16) {
        let query = format!(
            "
            DELETE FROM chaindexing_nodes
            WHERE id NOT IN (
                SELECT id
                FROM chaindexing_nodes
                ORDER BY id DESC
                LIMIT {prune_size}
            )
            "
        );

        Self::execute_raw_query(client, &query).await;
    }
}

#[async_trait::async_trait]
impl LoadsDataWithRawQuery for PostgresRepo {
    async fn load_latest_events<'a>(
        client: &Self::RawQueryClient,
        addresses: &Vec<String>,
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

fn join_i32s_with_comma(i32s: &[i32]) -> String {
    i32s.iter().map(|id| id.to_string()).collect::<Vec<String>>().join(",")
}

fn join_strings_with_comma(strings: &[String]) -> String {
    strings.iter().map(|string| format!("'{string}'")).collect::<Vec<_>>().join(",")
}
