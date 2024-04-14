use crate::{Migratable, PostgresRepo, RepoMigrations, SQLikeMigrations};

impl RepoMigrations for PostgresRepo {
    fn create_nodes_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_nodes()
    }

    fn create_contract_addresses_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_contract_addresses()
    }
    fn drop_contract_addresses_migration() -> &'static [&'static str] {
        SQLikeMigrations::drop_contract_addresses()
    }

    fn create_events_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_events()
    }
    fn drop_events_migration() -> &'static [&'static str] {
        SQLikeMigrations::drop_events()
    }

    fn create_reorged_blocks_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_reorged_blocks()
    }
    fn drop_reorged_blocks_migration() -> &'static [&'static str] {
        SQLikeMigrations::drop_reorged_blocks()
    }

    fn create_root_states_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_root_states()
    }

    fn create_handler_subscriptions_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_handler_subscriptions()
    }

    fn nullify_handler_subscriptions_next_block_number_to_handle_from_migration(
    ) -> &'static [&'static str] {
        SQLikeMigrations::nullify_handler_subscriptions_next_block_number_to_handle_from()
    }

    fn drop_handler_subscriptions_migration() -> &'static [&'static str] {
        SQLikeMigrations::drop_handler_subscriptions()
    }
}

impl Migratable for PostgresRepo {}
