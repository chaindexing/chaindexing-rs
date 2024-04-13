use crate::{
    contracts, root, ChaindexingError, ChaindexingRepo, ChaindexingRepoConn,
    ChaindexingRepoRawQueryClient, Config, Contract, ExecutesWithRawQuery, LoadsDataWithRawQuery,
    Migratable, Repo, RepoMigrations,
};

pub async fn run<'a, S: Sync + Send + Clone>(
    config: &Config<S>,
    conn: &mut ChaindexingRepoConn<'a>,
    client: &ChaindexingRepoRawQueryClient,
) -> Result<(), ChaindexingError> {
    let Config {
        contracts,
        reset_count,
        reset_queries,
        ..
    } = config;

    run_root_migrations(client).await;
    maybe_reset(reset_count, reset_queries, contracts, client).await;
    run_internal_migrations(client).await;
    run_migrations_for_contract_states(client, contracts).await;

    let contract_addresses: Vec<_> =
        contracts.clone().into_iter().flat_map(|c| c.addresses).collect();
    ChaindexingRepo::upsert_contract_addresses(conn, &contract_addresses).await;

    let event_handler_subscriptions = contracts::get_all_event_handler_subscriptions(contracts);
    ChaindexingRepo::create_event_handler_subscriptions(client, &event_handler_subscriptions).await;

    Ok(())
}

pub async fn run_root_migrations(client: &ChaindexingRepoRawQueryClient) {
    ChaindexingRepo::migrate(
        client,
        ChaindexingRepo::create_root_states_migration().to_vec(),
    )
    .await;
    ChaindexingRepo::prune_root_states(client, root::states::MAX_COUNT).await;
}

pub async fn maybe_reset<S: Send + Sync + Clone>(
    reset_count: &u64,
    reset_queries: &Vec<String>,
    contracts: &[Contract<S>],
    client: &ChaindexingRepoRawQueryClient,
) {
    let reset_count = *reset_count;
    let root_state = ChaindexingRepo::load_last_root_state(client).await;
    let last_reset_count = root_state.clone().map(|rs| rs.reset_count).unwrap_or(0);

    if reset_count > last_reset_count {
        reset_internal_migrations(client).await;
        reset_migrations_for_contract_states(client, contracts).await;
        run_user_reset_queries(client, reset_queries).await;

        let new_root_state = root_state.unwrap().with_new_reset_count(reset_count);
        ChaindexingRepo::create_root_state(client, &new_root_state).await;
    }
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
