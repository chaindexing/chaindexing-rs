use std::collections::HashMap;
use std::fmt::Debug;

use crate::handlers::{HandlerContext, PureHandlerContext};
use crate::{ChaindexingRepoTxnClient, Event};

use super::filters::Filters;
use super::state;
use super::state::read_many;
use super::state_versions::StateVersion;
use super::state_views::StateView;
use super::updates::Updates;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// States derived from different contracts within a chain
#[crate::augmenting_std::async_trait]
pub trait ChainState: DeserializeOwned + Serialize + Clone + Debug + Sync + Send + 'static {
    /// Table of the state as specified in StateMigrations
    fn table_name() -> &'static str;

    /// Inserts state in the state's table
    async fn create<'a, 'b>(&self, context: &PureHandlerContext<'a, 'b>) {
        state::create(Self::table_name(), &state::to_view(self), context).await;
    }

    /// Returns a single state matching filters. Panics if there are multiple.
    async fn read_one<'a, C: HandlerContext<'a>>(filters: &Filters, context: &C) -> Option<Self> {
        Self::read_many(filters, context).await.first().cloned()
    }

    /// Returns states matching filters
    async fn read_many<'a, C: HandlerContext<'a>>(filters: &Filters, context: &C) -> Vec<Self> {
        read_many(filters, context, Self::table_name()).await
    }

    /// Updates state with the specified updates
    async fn update<'a, 'b>(&self, updates: &Updates, context: &PureHandlerContext<'a, 'b>) {
        let event = &context.event;
        let client = context.repo_client;

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, event).await;

        let latest_state_version =
            StateVersion::update(&state_view, &updates.values, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    /// Deletes state from the state's table
    async fn delete<'a, 'b>(&self, context: &PureHandlerContext<'a, 'b>) {
        let event = &context.event;
        let client = context.repo_client;

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, event).await;

        let latest_state_version =
            StateVersion::delete(&state_view, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    fn to_view(&self) -> HashMap<String, String> {
        state::to_view(self)
    }

    async fn to_complete_view<'a>(
        &self,
        table_name: &str,
        client: &ChaindexingRepoTxnClient<'a>,
        event: &Event,
    ) -> HashMap<String, String> {
        let mut state_view = self.to_view();
        state_view.insert("chain_id".to_string(), event.chain_id.to_string());
        StateView::get_complete(&self.to_view(), table_name, client).await
    }
}
