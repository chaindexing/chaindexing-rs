use tokio_postgres::{types::ToSql, Client, NoTls, Transaction};

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
}

#[async_trait::async_trait]
impl LoadsDataWithRawQuery for PostgresRepo {
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
