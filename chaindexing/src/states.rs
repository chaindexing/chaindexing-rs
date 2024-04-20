//! # States
//! Any struct that can be serialized and deserialized while implementing
//! any state type, such as ContractState, ChainState, MultiChainState etc.
//! is a valid Chaindexing State
//!
//! ## Example
//!
//! ```rust,no_run
//! use chaindexing::states::{ContractState, StateMigrations};
//! use serde::{Deserialize, Serialize};

//! #[derive(Clone, Debug, Serialize, Deserialize)]
//! pub struct Nft {
//!     pub token_id: u32,
//!     pub owner_address: String,
//! }

//! impl ContractState for Nft {
//!     fn table_name() -> &'static str {
//!         "nfts"
//!     }
//! }

//! pub struct NftMigrations;

//! impl StateMigrations for NftMigrations {
//!     fn migrations(&self) -> &'static [&'static str] {
//!         &["CREATE TABLE IF NOT EXISTS nfts (
//!                 token_id INTEGER NOT NULL,
//!                 owner_address TEXT NOT NULL
//!             )"]
//!     }
//! }
//! ```
pub use migrations::StateMigrations;

use std::collections::HashMap;
use std::sync::Arc;

mod migrations;
mod state_versions;
mod state_views;

mod chain_state;
mod contract_state;
mod filters;
mod multi_chain_state;
mod state;
mod updates;

pub use filters::Filters;
pub use updates::Updates;

use crate::{
    ChaindexingRepo, ChaindexingRepoClient, ChaindexingRepoTxnClient, ExecutesWithRawQuery,
};

pub use chain_state::ChainState;
pub use contract_state::ContractState;
pub use multi_chain_state::MultiChainState;

use state_versions::{StateVersion, StateVersions, STATE_VERSIONS_TABLE_PREFIX};
use state_views::StateViews;

pub(crate) async fn backtrack_states<'a>(
    table_names: &Vec<String>,
    chain_id: i64,
    block_number: i64,
    client: &ChaindexingRepoTxnClient<'a>,
) {
    for table_name in table_names {
        let state_versions = StateVersions::get(block_number, chain_id, table_name, client).await;

        let state_version_ids = StateVersions::get_ids(&state_versions);
        StateVersions::delete_by_ids(&state_version_ids, table_name, client).await;

        let state_version_group_ids = StateVersions::get_group_ids(&state_versions);
        StateViews::refresh(&state_version_group_ids, table_name, client).await;
    }
}

pub(crate) async fn prune_state_versions(
    table_names: &Vec<String>,
    client: &ChaindexingRepoClient,
    min_block_number: u64,
    chain_id: u64,
) {
    for table_name in table_names {
        let state_version_table_name = StateVersion::table_name(table_name);

        ChaindexingRepo::execute(
            client,
            &format!(
                "
            DELETE FROM {state_version_table_name}
            WHERE block_number < {min_block_number} 
            AND chain_id = {chain_id}
            "
            ),
        )
        .await;
    }
}

pub(crate) fn get_all_table_names(state_migrations: &[Arc<dyn StateMigrations>]) -> Vec<String> {
    state_migrations
        .iter()
        .flat_map(|state_migration| state_migration.get_table_names())
        .collect()
}

pub(crate) fn to_columns_and_values(state: &HashMap<String, String>) -> (Vec<String>, Vec<String>) {
    state.iter().fold(
        (vec![], vec![]),
        |(mut columns, mut values), (column, value)| {
            columns.push(column.to_string());
            values.push(format!("'{value}'"));

            (columns, values)
        },
    )
}

pub(crate) fn to_and_filters(
    state: &HashMap<impl ToString + Send, impl ToString + Send>,
) -> String {
    let filters = state.iter().fold(vec![], |mut filters, (column, value)| {
        let column = column.to_string();
        let value = value.to_string();
        filters.push(format!("{column} = '{value}'"));

        filters
    });

    filters.join(" AND ")
}

pub(crate) fn serde_map_to_string_map(
    serde_map: &HashMap<impl AsRef<str>, serde_json::Value>,
) -> HashMap<String, String> {
    serde_map.iter().fold(HashMap::new(), |mut map, (key, value)| {
        if !value.is_null() {
            if value.is_object() {
                map.insert(key.as_ref().to_owned(), value.to_string());
            } else {
                map.insert(key.as_ref().to_owned(), value.to_string().replace('\"', ""));
            }
        }

        map
    })
}
