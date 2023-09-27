use tokio_postgres::{types::ToSql, Client, NoTls};

use crate::{ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo};
use serde::de::DeserializeOwned;

pub type PostgresRepoRawQueryClient = Client;

#[async_trait::async_trait]
impl HasRawQueryClient for PostgresRepo {
    type RawQueryClient = Client;
    async fn get_raw_query_client(&self) -> Self::RawQueryClient {
        let (client, conn) = tokio_postgres::connect(&self.url, NoTls).await.unwrap();

        tokio::spawn(async move { conn.await.map_err(|e| eprintln!("connection error: {}", e)) });

        client
    }
}

#[async_trait::async_trait]
impl ExecutesWithRawQuery for PostgresRepo {
    async fn execute_raw_query(client: &Self::RawQueryClient, query: &str) {
        client.execute(query, &[] as &[&(dyn ToSql + Sync)]).await.unwrap();
    }
}

#[async_trait::async_trait]
impl LoadsDataWithRawQuery for PostgresRepo {
    async fn load_data_from_raw_query<Data: Send + DeserializeOwned>(
        client: &Self::RawQueryClient,
        query: &str,
    ) -> Vec<Data> {
        let json_aggregate = get_json_aggregate(client, query).await;

        if json_aggregate.as_object().is_some() {
            serde_json::from_value(json_aggregate).unwrap()
        } else {
            vec![]
        }
    }
}

async fn get_json_aggregate(client: &PostgresRepoRawQueryClient, query: &str) -> serde_json::Value {
    let query = format!(
        "WITH result AS ({}) SELECT COALESCE(json_agg(result), '[]'::json) FROM result",
        query
    );
    let rows = client.query(query.as_str(), &[]).await.unwrap();

    rows.first().unwrap().get(0)
}
