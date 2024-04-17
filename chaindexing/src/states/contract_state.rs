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

#[async_trait::async_trait]
pub trait ContractState:
    DeserializeOwned + Serialize + Clone + Debug + Sync + Send + 'static
{
    fn table_name() -> &'static str;

    async fn create<'a, 'b>(&self, context: &PureHandlerContext<'a, 'b>) {
        state::create(Self::table_name(), &state::to_view(self), context).await;
    }

    async fn read_one<'a, C: HandlerContext<'a>>(filters: &Filters, context: &C) -> Option<Self> {
        Self::read_many(filters, context).await.first().cloned()
    }

    async fn read_many<'a, C: HandlerContext<'a>>(filters: &Filters, context: &C) -> Vec<Self> {
        read_many(filters, context, Self::table_name()).await
    }

    async fn update<'a, 'b>(&self, updates: &Updates, context: &PureHandlerContext<'a, 'b>) {
        let event = &context.event;
        let client = context.repo_client;

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, event).await;

        let latest_state_version =
            StateVersion::update(&state_view, &updates.values, table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

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
        state_view.insert(
            "contract_address".to_string(),
            event.contract_address.to_owned(),
        );
        StateView::get_complete(&state_view, table_name, client).await
    }
}
