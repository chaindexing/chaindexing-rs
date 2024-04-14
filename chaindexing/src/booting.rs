use crate::{
    contracts, root, ChaindexingError, ChaindexingRepo, ChaindexingRepoConn,
    ChaindexingRepoRawQueryClient, Config, Contract, ExecutesWithRawQuery, LoadsDataWithRawQuery,
    Migratable, Repo, RepoMigrations,
};

pub async fn setup_nodes<S: Sync + Send + Clone>(
    config: &Config<S>,
    client: &ChaindexingRepoRawQueryClient,
) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::create_nodes_migration().to_vec()).await;
    ChaindexingRepo::prune_nodes(client, config.max_concurrent_node_count).await;
}

pub async fn setup<'a, S: Sync + Send + Clone>(
    config: &Config<S>,
    conn: &mut ChaindexingRepoConn<'a>,
    client: &ChaindexingRepoRawQueryClient,
) -> Result<(), ChaindexingError> {
    let Config {
        contracts,
        reset_count,
        reset_including_side_effects_count,
        reset_queries,
        ..
    } = config;

    run_root_migrations(client).await;
    maybe_reset(
        *reset_count,
        *reset_including_side_effects_count,
        reset_queries,
        contracts,
        client,
    )
    .await;
    run_internal_migrations(client).await;
    run_migrations_for_contract_states(client, contracts).await;

    let contract_addresses: Vec<_> =
        contracts.clone().into_iter().flat_map(|c| c.addresses).collect();
    ChaindexingRepo::upsert_contract_addresses(conn, &contract_addresses).await;

    let handler_subscriptions = contracts::get_handler_subscriptions(contracts);
    ChaindexingRepo::create_handler_subscriptions(client, &handler_subscriptions).await;

    Ok(())
}

/// Root migrations should never be dropped
pub async fn run_root_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_root_states_migration().to_vec(),
    )
    .await;

    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_handler_subscriptions_migration().to_vec(),
    )
    .await;

    if ChaindexingRepo::load_last_root_state(client).await.is_none() {
        ChaindexingRepo::append_root_state(client, &Default::default()).await;
    }

    ChaindexingRepo::prune_root_states(client, root::states::MAX_COUNT).await;
}

pub async fn maybe_reset<S: Send + Sync + Clone>(
    reset_count: u64,
    reset_including_side_effects_count: u64,
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoRawQueryClient,
) {
    let mut root_state = ChaindexingRepo::load_last_root_state(client).await.unwrap();

    if reset_count > root_state.reset_count {
        reset(reset_queries, contracts, client).await;

        root_state.update_reset_count(reset_count);
    }

    if reset_including_side_effects_count > root_state.reset_including_side_effects_count {
        reset(reset_queries, contracts, client).await;

        ChaindexingRepo::migrate(
            client,
            ChaindexingRepo::nullify_handler_subscriptions_next_block_number_for_side_effects_migration().to_vec(),
        )
        .await;

        root_state.update_reset_including_side_effects_count(reset_including_side_effects_count);
    }

    ChaindexingRepo::append_root_state(client, &root_state).await;
}

pub async fn reset<S: Send + Sync + Clone>(
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoRawQueryClient,
) {
    reset_internal_migrations(client).await;
    reset_migrations_for_contract_states(client, contracts).await;
    run_user_reset_queries(client, reset_queries).await;

    let handler_subscriptions = contracts::get_handler_subscriptions(contracts);
    ChaindexingRepo::upsert_handler_subscriptions(client, &handler_subscriptions).await;
}

pub async fn run_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_internal_migrations()).await;
}
pub async fn reset_internal_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_reset_internal_migrations()).await;
}

pub async fn run_migrations_for_contract_states<S: Send + Sync + Clone>(
    client: &ChaindexingRepoRawQueryClient,
    contracts: &[Contract<S>],
) {
    for state_migration in contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_migrations()).await;
    }
}
pub async fn reset_migrations_for_contract_states<S: Send + Sync + Clone>(
    client: &ChaindexingRepoRawQueryClient,
    contracts: &[Contract<S>],
) {
    for state_migration in contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_reset_migrations()).await;
    }
}

async fn run_user_reset_queries(
    client: &ChaindexingRepoRawQueryClient,
    reset_queries: &Vec<String>,
) {
    for reset_query in reset_queries {
        ChaindexingRepo::execute_raw_query(client, reset_query).await;
    }
}
