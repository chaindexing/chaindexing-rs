use std::{collections::HashMap, fmt::Debug};

use crate::{
    ChaindexingRepo, ChaindexingRepoRawQueryClient, ExecutesWithRawQuery, LoadsDataWithRawQuery,
    Migration,
};
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub enum ContractStateError {
    AlreadyInserted,
    NotFound,
}

#[async_trait::async_trait]
pub trait ContractState:
    Into<HashMap<&'static str, String>> + Debug + Sync + Send + Clone + DeserializeOwned + 'static
{
    fn table_name() -> &'static str;

    async fn read(
        client: &ChaindexingRepoRawQueryClient,
        filters: HashMap<&'static str, String>,
    ) -> Result<Option<Self>, ContractStateError>
    where
        Self: Sized,
    {
        let states = Self::read_many(client, filters).await?;

        Ok(states.first().cloned())
    }

    async fn read_many(
        client: &ChaindexingRepoRawQueryClient,
        filters: HashMap<&'static str, String>,
    ) -> Result<Vec<Self>, ContractStateError>
    where
        Self: Sized,
    {
        let filters = filters.iter().fold(vec![], |mut filters, (column, value)| {
            filters.push(format!("{} = '{}'", column, value));

            filters
        });
        let filters = filters.join(" AND ");

        // TODO: Move to Repo level and prevent SQLInjection
        let raw_query = format!(
            "SELECT FROM {table_name} WHERE {filters}",
            table_name = Self::table_name(),
        );

        let states: Vec<Self> = ChaindexingRepo::load_data_from_raw_query(client, &raw_query).await;

        Ok(states)
    }

    async fn create(
        &self,
        client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        let unsaved_state: HashMap<&str, String> = self.clone().into();
        Self::append_row(unsaved_state, client, false).await?;
        Ok(())
    }

    async fn update(
        &self,
        client: &ChaindexingRepoRawQueryClient,
        updates: HashMap<&'static str, String>,
    ) -> Result<(), ContractStateError> {
        let mut state: HashMap<&str, String> = self.clone().into();
        state.extend(updates);
        let state: HashMap<&str, String> = self.clone().into();
        Self::append_row(state, client, false).await?;
        Ok(())
    }

    async fn delete(
        &self,
        client: &ChaindexingRepoRawQueryClient,
    ) -> Result<(), ContractStateError> {
        let state: HashMap<&str, String> = self.clone().into();
        Self::append_row(state, client, true).await?;
        Ok(())
    }

    async fn append_row(
        state: HashMap<&str, String>,
        client: &ChaindexingRepoRawQueryClient,
        is_deleted: bool,
    ) -> Result<(), ContractStateError> {
        let (columns, values) = state.into_iter().fold(
            (vec!["is_deleted"], vec![is_deleted.to_string()]),
            |(mut columns, mut values), (column, value)| {
                columns.push(column);
                values.push(format!("'{}'", value));

                (columns, values)
            },
        );
        let columns = columns.join(",");
        let values = values.join(",");

        // TODO: Move to Repo level and prevent SQLInjection
        let raw_query = format!(
            "INSERT INTO {table_name} ({columns}) VALUES ({values})",
            table_name = Self::table_name(),
            columns = columns,
            values = values
        );
        ChaindexingRepo::execute_raw_query(client, raw_query.as_str()).await;

        Ok(())
    }
}

// Since contract states are rebuildable from ground up, we can
// easen the type strictness for consumer applications.
pub trait ContractStateMigrations: Send + Sync {
    fn migrations(&self) -> Vec<Migration>;
}
