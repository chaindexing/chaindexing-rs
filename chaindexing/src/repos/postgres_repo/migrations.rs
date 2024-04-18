use crate::{Migratable, PostgresRepo, RepoMigrations, SQLikeMigrations};

impl RepoMigrations for PostgresRepo {
    fn create_nodes_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_nodes()
    }

    fn create_contract_addresses_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_contract_addresses()
    }
    fn restart_ingest_and_handlers_next_block_numbers_migration() -> &'static [&'static str] {
        SQLikeMigrations::restart_ingest_and_handlers_next_block_numbers()
    }
    fn zero_next_block_number_for_side_effects_migration() -> &'static [&'static str] {
        SQLikeMigrations::zero_next_block_number_for_side_effects()
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
}

impl Migratable for PostgresRepo {}
