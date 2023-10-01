use crate::{Migratable, PostgresRepo, RepoMigrations, SQLikeMigrations};

impl RepoMigrations for PostgresRepo {
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

    fn create_reset_counts_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_reset_counts()
    }
}

impl Migratable for PostgresRepo {}
