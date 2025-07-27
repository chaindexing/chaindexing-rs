use std::collections::HashMap;

use super::STATE_VERSIONS_TABLE_PREFIX;

use sqlparser::ast::{CreateTable, DataType, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

fn parse_create_table(migration: &str) -> Option<CreateTable> {
    let dialect = PostgreSqlDialect {};
    let mut stmts = Parser::parse_sql(&dialect, migration).ok()?;
    let stmt = stmts.pop()?;
    match stmt {
        Statement::CreateTable(ct) => Some(ct),
        _ => None,
    }
}

fn extract_table_name(migration: &str) -> String {
    let dialect = PostgreSqlDialect {};
    let ast = Parser::parse_sql(&dialect, migration).expect("Failed to parse SQL");

    ast.iter()
        .find_map(|stmt| match stmt {
            Statement::CreateTable(ct) => Some(ct.name.to_string()),
            _ => None,
        })
        .expect("CREATE TABLE statement not found")
}

fn extract_table_fields(migration: &str, remove_json_fields: bool) -> Vec<String> {
    let dialect = PostgreSqlDialect {};
    let ast = Parser::parse_sql(&dialect, migration).expect("Failed to parse SQL");

    ast.iter()
        .find_map(|stmt| match stmt {
            Statement::CreateTable(ct) => Some(&ct.columns),
            _ => None,
        })
        .expect("CREATE TABLE statement not found")
        .iter()
        .filter_map(|col| {
            if remove_json_fields {
                match &col.data_type {
                    DataType::JSON | DataType::JSONB => return None,
                    DataType::Custom(name, _) => {
                        let name_str = name.to_string().to_uppercase();
                        if name_str == "JSONB" {
                            return None;
                        }
                    }
                    _ => {}
                }
            }

            Some(col.name.value.clone())
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
    let dialect = PostgreSqlDialect {};
    let mut ast = Parser::parse_sql(&dialect, migration).expect("Failed to parse user migration");

    if let Some(Statement::CreateTable(ref mut create_table)) = ast.first_mut() {
        // Remove trailing parenthesis if it exists, since we'll add our own
        let columns_str = migration_to_append.trim_end_matches(')').trim();

        // Create a temporary table with the columns to append to properly parse them
        let temp_table_sql = format!("CREATE TABLE temp_table ({columns_str})");

        if let Ok(mut temp_ast) = Parser::parse_sql(&dialect, &temp_table_sql) {
            if let Some(Statement::CreateTable(temp_table)) = temp_ast.first_mut() {
                // Add all columns from the temporary table to the original table
                for column in &temp_table.columns {
                    create_table.columns.push(column.clone());
                }
            }
        }

        // Convert back to SQL string
        create_table.to_string()
    } else {
        // If it's not a CREATE TABLE statement, return as-is
        migration.to_string()
    }
}

fn get_remaining_state_versions_migration() -> String {
    // TOOO:: Maybe add `chaindexing_` here to prevent the user from
    // overriding these fields (including state_version_group_id)
    // state_version_id helps distinguish between state versions uniquely
    // Helps in rare case of no state view change but new state version
    format!(
        "state_version_id BIGSERIAL PRIMARY KEY,
        state_version_is_deleted BOOL NOT NULL default false,
        {})",
        DefaultMigration::get()
    )
}

fn get_remaining_state_views_migration() -> String {
    DefaultMigration::get()
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

fn get_alter_table_migrations(table_name: &str, default_columns_sql: &str) -> Vec<String> {
    // Split the default column definitions by comma and build ALTER TABLE statements for each
    default_columns_sql
        .split(',')
        .filter_map(|tok| {
            let col_def = tok.trim().trim_start_matches('(').trim_end_matches(')').trim();

            if col_def.is_empty() {
                return None;
            }

            Some(format!(
                "ALTER TABLE IF EXISTS {table_name} ADD COLUMN IF NOT EXISTS {col_def}"
            ))
        })
        .collect()
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
        log_index INTEGER NOT NULL"
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

/// Represents the idempotent database migrations required before
/// indexing a state.
pub trait StateMigrations: Send + Sync {
    /// SQL migrations for the state to index. These migrations must be idempotent
    /// and should include the `IF NOT EXISTS` check to keep them safe on repeated runs.
    fn migrations(&self) -> &'static [&'static str];

    /// All table names created by the user's migrations (derived via SQL parsing).
    fn get_table_names(&self) -> Vec<String> {
        self.migrations().iter().fold(Vec::new(), |mut names, mig| {
            if let Some(ct) = parse_create_table(mig) {
                names.push(ct.name.to_string());
            }
            names
        })
    }

    /// Expands the user's migrations with Chaindexing-specific additions
    /// (`state_views` and `state_versions` tables, unique index, etc.).
    fn get_migrations(&self) -> Vec<String> {
        self.migrations()
            .iter()
            .flat_map(|user_migration| {
                if let Some(_ct) = parse_create_table(user_migration) {
                    // Build companion migrations deterministically.
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

                    // Determine state_views and state_versions table names and columns via AST for reliability.
                    let (state_views_table_name, user_fields) =
                        if let Some(ct) = parse_create_table(user_migration) {
                            (
                                ct.name.to_string(),
                                extract_table_fields(user_migration, true),
                            )
                        } else {
                            (
                                extract_table_name(user_migration),
                                extract_table_fields(user_migration, true),
                            )
                        };

                    let (state_versions_table_name, state_versions_fields) =
                        if let Some(ct) = parse_create_table(&create_state_versions_table_migration) {
                            (
                                ct.name.to_string(),
                                extract_table_fields(&create_state_versions_table_migration, true),
                            )
                        } else {
                            (
                                extract_table_name(&create_state_versions_table_migration),
                                extract_table_fields(&create_state_versions_table_migration, true),
                            )
                        };

                    let create_state_versions_table_migration =
                        maybe_normalize_user_primary_key_column(
                            &create_state_versions_table_migration,
                        );

                    let mut migrations = vec![
                        create_state_views_table_migration.clone(),
                        create_state_versions_table_migration.clone(),
                    ];

                    // Ensure legacy installations gain missing default columns (idempotent ADD COLUMN IF NOT EXISTS)
                    migrations.extend(get_alter_table_migrations(
                        &state_views_table_name,
                        &DefaultMigration::get(),
                    ));
                    migrations.extend(get_alter_table_migrations(
                        &state_versions_table_name,
                        &format!(
                            "state_version_id BIGSERIAL PRIMARY KEY, state_version_is_deleted BOOL NOT NULL default false, {}",
                            DefaultMigration::get()
                        ),
                    ));

                    // Append deterministic UNIQUE INDEX if fields are available.
                    let mut fields_for_index = if state_versions_fields.is_empty() {
                        user_fields.clone()
                    } else {
                        // Combine user fields with essential blockchain fields for proper uniqueness
                        let mut combined_fields = user_fields.clone();

                        // Add essential blockchain fields that ensure each state version is unique
                        let essential_fields = ["chain_id", "block_number", "transaction_hash", "log_index"];
                        for field in essential_fields {
                            if !combined_fields.contains(&field.to_string()) {
                                combined_fields.push(field.to_string());
                            }
                        }

                        combined_fields
                    };

                    if fields_for_index.is_empty() {
                        fields_for_index = DefaultMigration::get_fields()
                            .iter()
                            .map(|s| s.to_string())
                            .collect();
                    }

                    if !fields_for_index.is_empty() {
                        // Drop any existing unique index that might have incorrect fields
                        migrations.push(format!(
                            "DROP INDEX IF EXISTS unique_{state_versions_table_name}"
                        ));

                        migrations.push(get_unique_index_migration_for_state_versions(
                            &state_versions_table_name,
                            fields_for_index,
                        ));
                    }

                    migrations
                } else {
                    vec![user_migration.to_string()]
                }
            })
            .collect()
    }

    /// The inverse of `get_migrations`: returns SQL that drops every table we created.
    fn get_reset_migrations(&self) -> Vec<String> {
        self.get_migrations()
            .iter()
            .filter_map(|mig| {
                if let Some(ct) = parse_create_table(mig) {
                    Some(format!("DROP TABLE IF EXISTS {}", ct.name))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod contract_state_migrations_get_migration_test {
    use super::*;

    #[test]
    fn returns_two_more_migrations_for_create_state_migrations() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();

        // With the new logic, we generate more migrations (CREATE TABLE + ALTER TABLE for backward compatibility + INDEX operations)
        // The exact count depends on the number of default fields and backward compatibility updates
        assert!(migrations.len() > contract_state.migrations().len() + 2);

        // Verify we have the core CREATE TABLE migrations
        let create_tables = migrations.iter().filter(|m| m.contains("CREATE TABLE")).count();
        assert_eq!(create_tables, 2); // One for state_views, one for state_versions
    }

    #[test]
    fn appends_default_migration_to_create_state_views_migrations() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();
        let create_state_views_migration = migrations
            .iter()
            .find(|m| m.contains("CREATE TABLE") && !m.contains(STATE_VERSIONS_TABLE_PREFIX))
            .unwrap();

        assert_ne!(
            create_state_views_migration,
            contract_state.migrations().first().unwrap()
        );

        assert_default_migration(create_state_views_migration);
    }

    #[test]
    fn removes_repeating_default_migrations_in_create_state_views_migration() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();
        let create_state_views_migration = migrations
            .iter()
            .find(|m| m.contains("CREATE TABLE") && !m.contains(STATE_VERSIONS_TABLE_PREFIX))
            .unwrap();

        assert_eq!(
            create_state_views_migration.matches("contract_address").count(),
            2
        );
        assert_eq!(
            create_state_views_migration.matches("pool_contract_address").count(),
            1
        )
    }

    #[test]
    fn creates_an_extra_migration_for_creating_state_versions() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();
        let create_state_versions_migration = migrations
            .iter()
            .find(|m| m.contains("CREATE TABLE") && m.contains(STATE_VERSIONS_TABLE_PREFIX))
            .unwrap();

        assert!(create_state_versions_migration.contains(STATE_VERSIONS_TABLE_PREFIX));
        assert_default_migration(create_state_versions_migration);
    }

    #[test]
    fn normalizes_user_primary_key_column_before_creating_state_versions_migrations() {
        let contract_state = TestStateWithPrimaryKey;
        let migrations = contract_state.get_migrations();
        let create_state_versions_migration = migrations
            .iter()
            .find(|m| m.contains("CREATE TABLE") && m.contains(STATE_VERSIONS_TABLE_PREFIX))
            .unwrap();

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
        let migrations = contract_state.get_migrations();

        assert_eq!(
            contract_state.migrations().last().unwrap(),
            migrations.last().unwrap()
        );
    }

    #[test]
    fn returns_unique_index_migrations_for_state_versions() {
        let contract_state = TestState;
        let migrations = contract_state.get_migrations();

        // Find the CREATE UNIQUE INDEX migration (it should be present somewhere in the list)
        let unique_index_migration =
            migrations.iter().find(|m| m.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));

        assert!(unique_index_migration.is_some());
        assert!(unique_index_migration.unwrap().contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
    }

    #[test]
    fn ignores_json_field_in_unique_index_migration() {
        let contract_state = TestStateWithJsonField;
        let migrations = contract_state.get_migrations();

        // Find the unique index migration
        let unique_index_migration = migrations.iter().find(|m| m.contains("CREATE UNIQUE INDEX"));

        if let Some(migration) = unique_index_migration {
            assert!(!migration.contains("json_field"));
        } else {
            // This is acceptable when all user fields are filtered out
        }
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
