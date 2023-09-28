use crate::{Migratable, PostgresRepo, RepoMigrations, SQLikeMigrations};

impl RepoMigrations for PostgresRepo {
    fn create_contract_addresses_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_contract_addresses()
    }

    fn create_events_migration() -> &'static [&'static str] {
        SQLikeMigrations::create_events()
    }
}

impl Migratable for PostgresRepo {}
