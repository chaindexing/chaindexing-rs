use crate::{
    contracts, root, ChaindexingError, ChaindexingRepo, ChaindexingRepoClient, Config, Contract,
    ExecutesWithRawQuery, LoadsDataWithRawQuery, Migratable, RepoMigrations,
};

pub async fn setup_nodes<S: Sync + Send + Clone>(
    config: &Config<S>,
    client: &ChaindexingRepoClient,
) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::create_nodes_migration().to_vec()).await;
    ChaindexingRepo::prune_nodes(client, config.max_concurrent_node_count).await;
}

pub async fn setup<'a, S: Sync + Send + Clone>(
    Config {
        contracts,
        reset_count,
        reset_including_side_effects_count,
        reset_queries,
        ..
    }: &Config<S>,
    client: &ChaindexingRepoClient,
) -> Result<(), ChaindexingError> {
    setup_root(client).await;

    maybe_reset(
        *reset_count,
        *reset_including_side_effects_count,
        reset_queries,
        contracts,
        client,
    )
    .await;

    run_internal_migrations(client).await;
    run_user_migrations(client, contracts).await;

    let contract_addresses: Vec<_> =
        contracts.clone().into_iter().flat_map(|c| c.addresses).collect();
    ChaindexingRepo::create_contract_addresses(client, &contract_addresses).await;

    Ok(())
}

/// Root migrations are immutable and should never really be dropped
async fn setup_root(client: &ChaindexingRepoClient) {
    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_root_states_migration().to_vec(),
    )
    .await;

    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_contract_addresses_migration().to_vec(),
    )
    .await;

    if ChaindexingRepo::load_last_root_state(client).await.is_none() {
        ChaindexingRepo::append_root_state(client, &Default::default()).await;
    }

    ChaindexingRepo::prune_root_states(client, root::states::MAX_COUNT).await;
}

async fn maybe_reset<S: Send + Sync + Clone>(
    reset_count: u64,
    reset_including_side_effects_count: u64,
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoClient,
) {
    let mut root_state = ChaindexingRepo::load_last_root_state(client).await.unwrap();

    let should_reset_normally = reset_count > root_state.reset_count;
    let should_reset_including_side_effects =
        reset_including_side_effects_count > root_state.reset_including_side_effects_count;

    if should_reset_normally {
        reset(reset_queries, contracts, client).await;

        root_state.update_reset_count(reset_count);
    }

    if should_reset_including_side_effects {
        reset(reset_queries, contracts, client).await;

        ChaindexingRepo::migrate(
            client,
            ChaindexingRepo::zero_next_block_number_for_side_effects_migration().to_vec(),
        )
        .await;

        root_state.update_reset_including_side_effects_count(reset_including_side_effects_count);
    }

    let reset_happened = should_reset_normally || should_reset_including_side_effects;
    if reset_happened {
        ChaindexingRepo::append_root_state(client, &root_state).await;
    }
}

async fn reset<S: Send + Sync + Clone>(
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoClient,
) {
    reset_internal_migrations(client).await;
    reset_user_migrations(client, contracts).await;
    run_user_reset_queries(client, reset_queries).await;
}

pub async fn run_internal_migrations(client: &ChaindexingRepoClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_internal_migrations()).await;
}
async fn reset_internal_migrations(client: &ChaindexingRepoClient) {
    ChaindexingRepo::migrate(client, ChaindexingRepo::get_reset_internal_migrations()).await;
}

pub async fn run_user_migrations<S: Send + Sync + Clone>(
    client: &ChaindexingRepoClient,
    contracts: &[Contract<S>],
) {
    for state_migration in contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_migrations()).await;
    }
}
async fn reset_user_migrations<S: Send + Sync + Clone>(
    client: &ChaindexingRepoClient,
    contracts: &[Contract<S>],
) {
    for state_migration in contracts::get_state_migrations(contracts) {
        ChaindexingRepo::migrate(client, state_migration.get_reset_migrations()).await;
    }
}
async fn run_user_reset_queries(client: &ChaindexingRepoClient, reset_queries: &Vec<String>) {
    for reset_query in reset_queries {
        ChaindexingRepo::execute(client, reset_query).await;
    }
}
