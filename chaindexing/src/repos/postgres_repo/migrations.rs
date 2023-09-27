use crate::{Migratable, Migration, PostgresRepo, RepoMigrations, SQLikeMigrations};

impl RepoMigrations for PostgresRepo {
    fn create_contract_addresses_migration() -> &'static [Migration] {
        SQLikeMigrations::create_contract_addresses()
    }

    fn create_events_migration() -> &'static [Migration] {
        SQLikeMigrations::create_events()
    }
}

impl Migratable for PostgresRepo {}
