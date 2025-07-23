use std::collections::HashMap;

use super::STATE_VERSIONS_TABLE_PREFIX;

/// Represents the idempotent database migrations required before
/// indexing a state.
pub trait StateMigrations: Send + Sync {
    /// SQL migrations for the state to index. These migrations must be idempotent
    /// and will require using the 'IF NOT EXISTS` check
    ///
    /// # Example
    /// ```ignore
    /// fn migrations(&self) -> &'static [&'static str] {
    ///     &["CREATE TABLE IF NOT EXISTS nfts (
    ///        token_id INTEGER NOT NULL,
    ///        owner_address TEXT NOT NULL
    ///        )"]
    ///  }
    /// ```
    fn migrations(&self) -> &'static [&'static str];

    fn get_table_names(&self) -> Vec<String> {
        self.migrations().iter().fold(vec![], |mut table_names, migration| {
            if migration.starts_with("CREATE TABLE IF NOT EXISTS") {
                let table_name = extract_table_name(migration);
                table_names.push(table_name)
            }

            table_names
        })
    }

    fn get_migrations(&self) -> Vec<String> {
        self.migrations()
            .iter()
            .flat_map(|user_migration| {
                // NOTE: We intentionally skip strict validation here to allow the
                // end-user supply ANY valid SQL definition (including complex
                // types like ID/BIGSERIAL, TIMESTAMPTZ, ARRAY, NUMERIC(10,2), etc.)
                // Previously `validate_migration` rejected some of these
                // definitions in order to keep the SQL parser simple, but it
                // proved too restrictive in real-world use-cases.  By removing
                // this early validation we accept the responsibility of the
                // underlying database to validate the statement semantics.

                if user_migration.starts_with("CREATE TABLE IF NOT EXISTS") {
                    let create_state_views_table_migration =
                        append_migration(user_migration, &get_remaining_state_views_migration());
                    let create_state_views_table_migration =
                        DefaultMigration::remove_repeating_occurrences(
                            &create_state_views_table_migration,
                        );

                    let create_state_versions_table_migration =
                        append_migration(user_migration, &get_remaining_state_versions_migration());
                    let create_state_versions_table_migration =
                        set_state_versions_table_name(&create_state_versions_table_migration);
                    let create_state_versions_table_migration =
                        DefaultMigration::remove_repeating_occurrences(
                            &create_state_versions_table_migration,
                        );

                    let state_versions_table_name =
                        extract_table_name(&create_state_versions_table_migration);
                    let state_versions_fields =
                        extract_table_fields(&create_state_versions_table_migration, true);

                    let create_state_versions_table_migration =
                        maybe_normalize_user_primary_key_column(
                            &create_state_versions_table_migration,
                        );

                    let mut migrations_to_return = vec![
                        create_state_views_table_migration,
                        create_state_versions_table_migration,
                    ];

                    // Attempt to build a UNIQUE INDEX only when we have been able to
                    // safely extract column names from the original migration.
                    if !state_versions_fields.is_empty() {
                        migrations_to_return.push(
                            get_unique_index_migration_for_state_versions(
                                &state_versions_table_name,
                                state_versions_fields,
                            ),
                        );
                    }

                    migrations_to_return
                } else {
                    vec![user_migration.to_string()]
                }
            })
            .collect()
    }

    fn get_reset_migrations(&self) -> Vec<String> {
        self.get_migrations()
            .iter()
            .filter_map(|migration| {
                if migration.starts_with("CREATE TABLE IF NOT EXISTS") {
                    let table_name = extract_table_name(migration);

                    Some(format!("DROP TABLE IF EXISTS {table_name}"))
                } else {
                    None
                }
            })
            .collect()
    }
}

fn extract_table_name(migration: &str) -> String {
    migration
        .replace("CREATE TABLE IF NOT EXISTS", "")
        .split('(')
        .collect::<Vec<&str>>()
        .first()
        .unwrap()
        .trim()
        .to_string()
}

fn extract_table_fields(migration: &str, remove_json_fields: bool) -> Vec<String> {
    migration
        .replace(')', "")
        .split('(')
        .collect::<Vec<&str>>()
        .last()
        .unwrap()
        .split(',')
        .filter_map(|field| {
            // Skip JSON fields if requested
            if remove_json_fields && (field.contains("JSON") || field.contains("JSONB")) {
                return None;
            }

            // The first whitespace-separated token is assumed to be the column name.
            // When the type itself contains commas (e.g. NUMERIC(10,2)) the split
            // above will incorrectly treat `2` as a separate `field`.  We guard
            // against this by checking that the candidate token starts with an
            // alphabetic character – legitimate column names must start with a
            // letter or an underscore.
            let token = field
                .split_ascii_whitespace()
                .collect::<Vec<&str>>()
                .first()
                .unwrap()
                .trim();

            if token.is_empty() {
                return None;
            }

            let first_char = token.chars().next().unwrap();
            if first_char.is_ascii_alphabetic() || first_char == '_' {
                Some(token.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn get_unique_index_migration_for_state_versions(
    table_name: &str,
    table_fields: Vec<String>,
) -> String {
    let table_fields: Vec<String> =
        table_fields.into_iter().filter(|f| f.as_str() != "state_version_id").collect();
    let fields_by_comma = table_fields.join(",");

    format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS unique_{table_name} ON {table_name}({fields_by_comma})"
    )
}

// Note: Runtime/database will now enforce semantic correctness of field types, so the previous
// `validate_migration` helper—used to forbid timestamp/date fields—has been removed.  This keeps
// the crate free of dead-code warnings while relying on the new `state_migrations!` macro for
// syntactic validation.

fn append_migration(migration: &str, migration_to_append: &str) -> String {
    let mut migration = migration.replace('\n', "");
    migration.push(',');
    migration.push_str(migration_to_append);
    migration
        .split_ascii_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .replace("),", ",")
        .replace("),,", ",")
        .replace(", ,", ",")
}

fn get_remaining_state_versions_migration() -> String {
    // TOOO:: Maybe add `chaindexing_` here to prevent the user from
    // overriding these fields (including state_version_group_id)
    // state_version_id helps distinguish between state versions uniquely
    // Helps in rare case of no state view change but new state version
    format!(
        "state_version_id BIGSERIAL PRIMARY KEY,
        state_version_is_deleted BOOL NOT NULL default false,
        {}
        ",
        DefaultMigration::get()
    )
}

fn get_remaining_state_views_migration() -> String {
    DefaultMigration::get().to_string()
}

fn set_state_versions_table_name(migration: &str) -> String {
    migration.replace(
        "CREATE TABLE IF NOT EXISTS ",
        format!("CREATE TABLE IF NOT EXISTS {STATE_VERSIONS_TABLE_PREFIX}",).as_str(),
    )
}

fn extract_migration_columns(migration: &str) -> Vec<String> {
    let mut migration_tokens = migration.split('(');
    let migration = migration_tokens.next_back().unwrap();
    let mut migration_tokens = migration.split(')');
    let migration = migration_tokens.next().unwrap();

    migration.split(',').fold(vec![], |mut migration_columns, migration_column| {
        migration_columns.push(migration_column.to_string());
        migration_columns
    })
}

fn filter_migration_columns_containing(migration: &str, to_match_with: &str) -> Vec<String> {
    extract_migration_columns(migration)
        .iter()
        .filter(|migration_column| migration_column.contains(to_match_with))
        .cloned()
        .collect()
}

fn maybe_normalize_user_primary_key_column(state_versions_migration: &str) -> String {
    let primary_key_columns =
        filter_migration_columns_containing(state_versions_migration, "PRIMARY KEY");

    if primary_key_columns.len() == 2 {
        let user_primary_key_column = format!(
            "{},",
            primary_key_columns.iter().find(|c| !c.contains("state_version_id")).unwrap()
        );
        let user_primary_key_column_replacement =
            user_primary_key_column.replace("PRIMARY KEY", "");

        state_versions_migration.replace(
            &user_primary_key_column,
            &user_primary_key_column_replacement,
        )
    } else {
        state_versions_migration.to_string()
    }
}

struct DefaultMigration;

impl DefaultMigration {
    pub fn get() -> String {
        "state_version_group_id UUID NOT NULL,
        contract_address VARCHAR NOT NULL,
        chain_id BIGINT NOT NULL,
        block_hash VARCHAR NOT NULL,
        block_number BIGINT NOT NULL,
        transaction_hash VARCHAR NOT NULL,
        transaction_index INTEGER NOT NULL,
        log_index INTEGER NOT NULL)"
            .to_string()
    }

    pub fn get_fields() -> &'static [&'static str] {
        &[
            "contract_address",
            "chain_id",
            "block_hash",
            "block_number",
            "transaction_hash",
            "transaction_index",
            "log_index",
        ]
    }

    fn remove_repeating_occurrences(migration: &str) -> String {
        let repeating_state_fields: Vec<_> = Self::get_fields()
            .iter()
            .filter(|field| migration.matches(*field).count() > 1)
            .collect();

        let mut repeating_state_fields_count = repeating_state_fields.iter().fold(
            HashMap::new(),
            |mut repeating_field_count, field| {
                repeating_field_count.insert(*field, 0_u8);

                repeating_field_count
            },
        );

        migration
            .split(',')
            .fold(vec![], |mut unique_migration_tokens, migration_token| {
                let migration_token_field =
                    migration_token.split_ascii_whitespace().next().unwrap();

                match repeating_state_fields
                    .iter()
                    .find(|field| (***field) == migration_token_field)
                {
                    Some(field) => {
                        let previous_count = repeating_state_fields_count.get(field).unwrap();

                        if *previous_count != 1 {
                            let new_count = previous_count + 1;
                            repeating_state_fields_count.insert(field, new_count);

                            unique_migration_tokens.push(migration_token)
                        }
                    }
                    None => unique_migration_tokens.push(migration_token),
                }

                unique_migration_tokens
            })
            .join(",")
    }
}

#[cfg(test)]
mod contract_state_migrations_get_migration_test {
    use super::*;

    #[test]
    fn returns_two_more_migrations_for_create_state_migrations() {
        let contract_state = TestState;

        assert_eq!(
            contract_state.get_migrations().len(),
            contract_state.migrations().len() + 2
        );
    }

    #[test]
    fn appends_default_migration_to_create_state_views_migrations() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();
        let create_state_migration = migrations.first().unwrap();

        assert_ne!(
            create_state_migration,
            contract_state.migrations().first().unwrap()
        );

        assert_default_migration(create_state_migration);
    }

    #[test]
    fn removes_repeating_default_migrations_in_create_state_views_migration() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();
        let create_state_migration = migrations.first().unwrap();

        assert_eq!(
            create_state_migration.matches("contract_address").count(),
            2
        );
        assert_eq!(
            create_state_migration.matches("pool_contract_address").count(),
            1
        )
    }

    #[test]
    fn creates_an_extra_migration_for_creating_state_versions() {
        let contract_state = TestState;
        let mut migrations = contract_state.get_migrations();
        migrations.pop();
        let create_state_versions_migration = migrations.last().unwrap();

        assert!(create_state_versions_migration.contains(STATE_VERSIONS_TABLE_PREFIX));
        assert_default_migration(create_state_versions_migration);
    }

    #[test]
    fn normalizes_user_primary_key_column_before_creating_state_versions_migrations() {
        let contract_state = TestStateWithPrimaryKey;
        let mut migrations = contract_state.get_migrations();
        migrations.pop();
        let create_state_versions_migration = migrations.last().unwrap();

        assert_eq!(
            create_state_versions_migration.matches("PRIMARY KEY").count(),
            1
        );
        assert_eq!(
            create_state_versions_migration.matches("id SERIAL PRIMARY KEY").count(),
            0
        );
        assert_eq!(
            create_state_versions_migration.matches("id SERIAL").count(),
            1
        );
    }

    fn assert_default_migration(migration: &str) {
        DefaultMigration::get_fields()
            .iter()
            .for_each(|field| assert!(migration.contains(field)));
    }

    #[test]
    fn returns_other_migrations_untouched() {
        let contract_state = TestState;

        assert_eq!(
            contract_state.migrations().last().unwrap(),
            contract_state.get_migrations().last().unwrap()
        );
    }

    #[test]
    fn returns_unique_index_migrations_for_state_versions() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();

        let unique_index_migration = migrations.get(2);

        assert!(unique_index_migration.is_some());
        assert!(unique_index_migration.unwrap().contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
    }

    #[test]
    fn ignores_json_field_in_unique_index_migration() {
        let contract_state = TestStateWithJsonField;
        let migrations = contract_state.get_migrations();

        let unique_index_migration = migrations.get(2);

        assert!(!unique_index_migration.unwrap().contains("json_field"));
    }

    struct TestState;

    impl StateMigrations for TestState {
        fn migrations(&self) -> &'static [&'static str] {
            &[
                "CREATE TABLE IF NOT EXISTS nft_states (
                      token_id INTEGER NOT NULL,
                      contract_address VARCHAR NOT NULL,
                      pool_contract_address VARCHAR NOT NULL,
                      owner_address VARCHAR NOT NULL
                  )",
                "UPDATE nft_states
                  SET owner_address = ''
                  WHERE owner_address IS NULL",
            ]
        }
    }

    struct TestStateWithPrimaryKey;

    impl StateMigrations for TestStateWithPrimaryKey {
        fn migrations(&self) -> &'static [&'static str] {
            &["CREATE TABLE IF NOT EXISTS nft_states (
                      id SERIAL PRIMARY KEY,
                      token_id INTEGER NOT NULL,
                      contract_address VARCHAR NOT NULL,
                      owner_address VARCHAR NOT NULL
                  )"]
        }
    }

    struct TestStateWithJsonField;

    impl StateMigrations for TestStateWithJsonField {
        fn migrations(&self) -> &'static [&'static str] {
            &["CREATE TABLE IF NOT EXISTS nft_states (
                      id SERIAL PRIMARY KEY,
                      token_id INTEGER NOT NULL,
                      json_field JSON DEFAULT '{}',
                  )"]
        }
    }
}
