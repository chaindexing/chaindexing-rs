use std::collections::HashMap;
use std::fmt::Debug;

use crate::handlers::{HandlerContext, PureHandlerContext};
use crate::{ChaindexingRepoRawQueryTxnClient, Event};

use super::filters::Filters;
use super::state;
use super::state::read_many;
use super::state_versions::StateVersion;
use super::state_views::StateView;
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

    async fn update<'a, 'b>(&self, updates: &Filters, context: &PureHandlerContext<'a, 'b>) {
        let event = &context.event;
        let client = context.raw_query_client;

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, event).await;

        Self::must_be_within_contract(&state_view, event);

        let latest_state_version =
            StateVersion::update(&state_view, &updates.get(event), table_name, event, client).await;
        StateView::refresh(&latest_state_version, table_name, client).await;
    }

    async fn delete<'a, 'b>(&self, context: &PureHandlerContext<'a, 'b>) {
        let event = &context.event;
        let client = context.raw_query_client;

        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, event).await;

        Self::must_be_within_contract(&state_view, event);

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
        client: &ChaindexingRepoRawQueryTxnClient<'a>,
        event: &Event,
    ) -> HashMap<String, String> {
        StateView::get_complete(&self.to_view(), table_name, client, event).await
    }

    fn must_be_within_contract(state_view: &HashMap<String, String>, event: &Event) {
        if state_view["contract_address"] != event.contract_address
            || state_view["chain_id"].parse::<i64>().unwrap() != event.chain_id
        {
            panic!("ContractStateError: Can't mutate state outside originating contract or chain")
        }
    }
}
