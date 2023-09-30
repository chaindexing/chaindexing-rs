use super::STATE_VERSIONS_TABLE_PREFIX;

// Since contract states are rebuildable from ground up, we can
// easen the type strictness for consumer applications.
pub trait ContractStateMigrations: Send + Sync {
    fn migrations(&self) -> Vec<&'static str>;

    fn get_migrations(&self) -> Vec<String> {
        self.migrations()
            .iter()
            .flat_map(|user_migration| {
                validate_migration(user_migration);

                if user_migration.starts_with("CREATE TABLE IF NOT EXISTS") {
                    // FUTURE TODO: Index user migration fields
                    // Index State Version fields
                    let create_state_table_migration = user_migration.to_owned().to_owned();

                    let create_state_versions_table_migration =
                        append_migration(&user_migration, &remaining_state_versions_migration());
                    let create_state_versions_table_migration =
                        set_state_versions_table_name(&create_state_versions_table_migration);

                    let state_versions_table_name =
                        extract_table_name(&create_state_versions_table_migration);
                    let state_versions_fields =
                        extract_table_fields(&create_state_versions_table_migration);

                    let state_versions_unique_index_migration =
                        unique_index_migration_for_state_versions(
                            state_versions_table_name,
                            state_versions_fields,
                        );

                    vec![
                        create_state_table_migration,
                        create_state_versions_table_migration,
                        state_versions_unique_index_migration,
                    ]
                } else {
                    vec![user_migration.to_string()]
                }
            })
            .collect()
    }
}

fn extract_table_name(migration: &str) -> String {
    migration
        .replace("CREATE TABLE IF NOT EXISTS", "")
        .split("(")
        .collect::<Vec<&str>>()
        .first()
        .unwrap()
        .trim()
        .to_string()
}

fn extract_table_fields(migration: &str) -> Vec<String> {
    migration
        .replace(")", "")
        .split("(")
        .collect::<Vec<&str>>()
        .last()
        .unwrap()
        .split(",")
        .map(|each_field| {
            each_field
                .split_ascii_whitespace()
                .collect::<Vec<&str>>()
                .first()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn unique_index_migration_for_state_versions(
    table_name: String,
    table_fields: Vec<String>,
) -> String {
    let table_fields: Vec<String> =
        table_fields.into_iter().filter(|f| f.as_str() != "state_version_id").collect();
    let fields_by_comma = table_fields.join(",");

    format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS unique_{table_name} ON {table_name}({fields_by_comma})"
    )
}

fn validate_migration(migration: &str) {
    let invalid_migration_keywords = [" timestamp", " timestampz", " date", " time"];

    invalid_migration_keywords.iter().for_each(|keyword| {
        if migration.to_lowercase().contains(keyword) {
            panic!("{keyword} tyoe of fields cannot be indexed")
        }
    });
}

fn append_migration(migration: &str, migration_to_append: &str) -> String {
    let mut migration = migration.replace("\n", "");
    migration.push_str(",");
    migration.push_str(migration_to_append);
    migration
        .split_ascii_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .replace("),", ",")
        .replace("),,", ",")
}

fn remaining_state_versions_migration() -> String {
    // No need for IDs, we will be using each field in the state
    // and delegate the responsibility of creating unique states
    // to the user, using whatever means necessary. The user can
    // decide to use the `contract_address`,
    format!(
        "state_version_id BIGSERIAL PRIMARY KEY,
        state_version_is_deleted BOOL NOT NULL default false,
        {}
        ",
        remaining_state_fields_migration()
    )
}

fn remaining_state_fields_migration() -> String {
    "state_version_contract_address TEXT NOT NULL,
    state_version_block_hash TEXT NOT NULL,
    state_version_block_number BIGINT NOT NULL,
    state_version_transaction_hash TEXT NOT NULL,
    state_version_transaction_index BIGINT NOT NULL,
    state_version_log_index BIGINT NOT NULL)"
        .to_string()
}

fn set_state_versions_table_name(migration: &str) -> String {
    migration.replace(
        "CREATE TABLE IF NOT EXISTS ",
        format!("CREATE TABLE IF NOT EXISTS {STATE_VERSIONS_TABLE_PREFIX}",).as_str(),
    )
}

#[cfg(test)]
mod contract_state_migrations_get_migration_test {
    use super::*;

    #[test]
    fn returns_two_more_migrations_for_create_state_migrations() {
        let contract_state = test_contract_state();

        assert_eq!(
            contract_state.get_migrations().len(),
            contract_state.migrations().len() + 2
        );
    }

    #[test]
    fn leaves_create_state_migrations_untouched() {
        let contract_state = test_contract_state();

        assert_eq!(
            contract_state.get_migrations().first().unwrap(),
            contract_state.migrations().first().unwrap()
        );
    }

    #[test]
    fn creates_an_extra_migration_for_creating_state_versions() {
        let contract_state = test_contract_state();
        let mut migrations = contract_state.get_migrations();
        migrations.pop();
        let create_state_versions_migration = migrations.last().unwrap();

        assert!(create_state_versions_migration.contains(STATE_VERSIONS_TABLE_PREFIX));
        assert_state_version_fields_in_migration(create_state_versions_migration);
    }

    fn assert_state_version_fields_in_migration(migration: &str) {
        const BLOCK_FIELDS: [&'static str; 5] = [
            "state_version_block_hash",
            "state_version_block_number",
            "state_version_transaction_hash",
            "state_version_transaction_index",
            "state_version_log_index",
        ];
        BLOCK_FIELDS.iter().for_each(|field| assert!(migration.contains(field)));
    }

    #[test]
    fn returns_other_migrations_untouched() {
        let contract_state = test_contract_state();

        assert_eq!(
            contract_state.migrations().last().unwrap(),
            contract_state.get_migrations().last().unwrap()
        );
    }

    fn test_contract_state() -> impl ContractStateMigrations {
        struct TestContractState;

        impl ContractStateMigrations for TestContractState {
            fn migrations(&self) -> Vec<&'static str> {
                vec![
                    "CREATE TABLE IF NOT EXISTS nft_states (
                      token_id INTEGER NOT NULL,
                      contract_address TEXT NOT NULL,
                      owner_address TEXT NOT NULL
                  )",
                    "UPDATE nft_states
                  SET owner_address = ''
                  WHERE owner_address IS NULL",
                ]
            }
        }

        TestContractState
    }
}
