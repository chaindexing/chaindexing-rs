use std::collections::HashMap;
use std::fmt::Debug;

use crate::handlers::{HandlerContext, PureHandlerContext};
use crate::ChaindexingRepoTxnClient;

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
        let event = context.event.clone();
        let client = context.repo_client;
        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client).await;
        let updates = updates.clone();
        let client = context.repo_client_for_mcs.clone();

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

    async fn delete<'a, 'b>(&self, context: &PureHandlerContext<'a, 'b>) {
        let event = context.event.clone();
        let client = context.repo_client;
        let table_name = Self::table_name();
        let state_view = self.to_complete_view(table_name, client).await;
        let client = context.repo_client_for_mcs.clone();

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
        client: &ChaindexingRepoTxnClient<'a>,
    ) -> HashMap<String, String> {
        StateView::get_complete(&self.to_view(), table_name, client).await
    }
}
