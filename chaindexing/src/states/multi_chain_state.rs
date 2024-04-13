use std::collections::HashMap;
use std::fmt::Debug;

use crate::event_handlers::EventHandlerContext;
use crate::{ChaindexingRepoRawQueryTxnClient, Event};

use super::filters::Filters;
use super::state::{self, read_many};
use super::state_versions::StateVersion;
use super::state_views::StateView;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// N/B: Indexing MultiChainStates must be Order-Agnostic
#[async_trait::async_trait]
pub trait MultiChainState:
    DeserializeOwned + Serialize + Clone + Debug + Sync + Send + 'static
{
    fn table_name() -> &'static str;

    async fn create<'a, S: Send + Sync + Clone>(&self, context: &EventHandlerContext<S>) {
        state::create(Self::table_name(), &state::to_view(self), context).await;
    }

    async fn read_one<'a, S: Send + Sync + Clone>(
        filters: &Filters,
        context: &EventHandlerContext<S>,
    ) -> Option<Self> {
        Self::read_many(filters, context).await.first().cloned()
    }

    async fn read_many<'a, S: Send + Sync + Clone>(
        filters: &Filters,
        context: &EventHandlerContext<S>,
    ) -> Vec<Self> {
        read_many(filters, context, Self::table_name()).await
    }

    async fn update<'a, 'b, 'life1: 'b, S: Send + Sync + Clone>(
        &self,
        updates: &'life1 Filters,
        context: &'life1 EventHandlerContext<'a, 'b, S>,
    ) {
        let event = context.event.clone();
        let client = context.raw_query_client;
        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, &event).await;
        let client = context.raw_query_client_for_mcs.clone();

        context
            .deferred_mutations_for_mcs
            .add(async move {
                let mut client = client.lock().await;

                let latest_state_version = StateVersion::update_without_txn(
                    &state_view,
                    &updates.get(&event),
                    table_name,
                    &event,
                    &mut client,
                )
                .await;
                StateView::refresh_without_txn(&latest_state_version, table_name, &client).await;
            })
            .await;
    }

    async fn delete<'a, 'b, 'life1: 'b, S: Send + Sync + Clone>(
        &self,
        context: &'life1 EventHandlerContext<S>,
    ) {
        let event = context.event.clone();
        let client = context.raw_query_client;
        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client, &event).await;
        let client = context.raw_query_client_for_mcs.clone();

        context
            .deferred_mutations_for_mcs
            .add(async move {
                let client = client.lock().await;

                let latest_state_version =
                    StateVersion::delete_without_txn(&state_view, table_name, &event, &client)
                        .await;
                StateView::refresh_without_txn(&latest_state_version, table_name, &client).await;
            })
            .await;
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
}
