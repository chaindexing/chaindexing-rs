use std::collections::HashMap;

use crate::{state_versions, state_view, Event};
use crate::{ChaindexingRepo, ChaindexingRepoRawQueryTxnClient};
use crate::{ExecutesWithRawQuery, LoadsDataWithRawQuery};

use super::state_versions::{StateVersion, StateVersions, STATE_VERSIONS_UNIQUE_FIELDS};
use super::{serde_map_to_string_map, to_and_filters, to_columns_and_values};

pub async fn refresh<'a>(
    state_version_group_ids: &[String],
    table_name: &str,
    client: &ChaindexingRepoRawQueryTxnClient<'a>,
) {
    let latest_state_versions =
        state_versions::get_latest(state_version_group_ids, table_name, client).await;

    for latest_state_version in latest_state_versions {
        state_view::refresh(&latest_state_version, table_name, client).await
    }
}
