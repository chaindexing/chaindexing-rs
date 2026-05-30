//! Experimental local backend prototypes.
//!
//! These APIs document and exercise local-backend behavior without making SQLite
//! a supported runtime backend. Postgres remains the production backend.

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LocalBackendGuarantee {
    InternalSchema,
    AtomicIngestionBatch,
    ReorgStatusTransitions,
    IdentityIdempotency,
    OrderedHandlerReads,
    OutboxLeasing,
    ReadOnlyInspection,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PrototypeStatus {
    ProvedInPrototype,
    ScopedForPrototype,
    NotYetProved,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PrototypeGuarantee {
    pub guarantee: LocalBackendGuarantee,
    pub status: PrototypeStatus,
    pub notes: &'static str,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalBackendPrototypeReport {
    pub backend: &'static str,
    pub guarantees: &'static [PrototypeGuarantee],
}

pub struct SqlitePrototype;

const SQLITE_GUARANTEES: &[PrototypeGuarantee] = &[
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::InternalSchema,
        status: PrototypeStatus::ProvedInPrototype,
        notes: "the prototype schema creates each internal table with SQLite-compatible types",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::AtomicIngestionBatch,
        status: PrototypeStatus::ProvedInPrototype,
        notes: "SQLite transactions roll back mixed block, scan, event, transaction, trace, and checkpoint writes",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::ReorgStatusTransitions,
        status: PrototypeStatus::ProvedInPrototype,
        notes: "prototype reorg statements mark canonical blocks, scans, events, transactions, and traces consistently",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::IdentityIdempotency,
        status: PrototypeStatus::ProvedInPrototype,
        notes: "prototype schema carries the same internal identity keys used by the Postgres backend",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::OrderedHandlerReads,
        status: PrototypeStatus::ScopedForPrototype,
        notes: "query shape is defined, but full state replay still needs a SQLite repo implementation",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::OutboxLeasing,
        status: PrototypeStatus::ScopedForPrototype,
        notes: "single-writer lease behavior is modeled; multi-process contention still needs stress testing",
    },
    PrototypeGuarantee {
        guarantee: LocalBackendGuarantee::ReadOnlyInspection,
        status: PrototypeStatus::ProvedInPrototype,
        notes: "inspection queries use portable status-scoped SELECTs over the internal tables",
    },
];

impl SqlitePrototype {
    pub fn report() -> LocalBackendPrototypeReport {
        LocalBackendPrototypeReport {
            backend: "sqlite",
            guarantees: SQLITE_GUARANTEES,
        }
    }

    pub fn internal_migrations() -> &'static [&'static str] {
        &[
            "CREATE TABLE IF NOT EXISTS chaindexing_root_states (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                reset_count INTEGER NOT NULL,
                reset_including_side_effects_count INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS chaindexing_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                last_active_at INTEGER DEFAULT (strftime('%s','now')),
                inserted_at INTEGER DEFAULT (strftime('%s','now'))
            )",
            "CREATE TABLE IF NOT EXISTS chaindexing_contract_addresses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL,
                contract_name TEXT NOT NULL,
                chain_id INTEGER NOT NULL,
                start_block_number INTEGER NOT NULL,
                next_block_number_to_ingest_from INTEGER NOT NULL,
                next_block_number_to_handle_from INTEGER NOT NULL,
                next_block_number_for_side_effects INTEGER DEFAULT 0
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_contract_addresses_chain_address_index
                ON chaindexing_contract_addresses(chain_id, address)",
            "CREATE TABLE IF NOT EXISTS chaindexing_events (
                id TEXT PRIMARY KEY,
                chain_id INTEGER NOT NULL,
                contract_address TEXT NOT NULL,
                contract_name TEXT NOT NULL,
                abi TEXT NOT NULL,
                parameters TEXT NOT NULL,
                topics TEXT NOT NULL,
                block_hash TEXT NOT NULL,
                block_number INTEGER NOT NULL,
                block_timestamp INTEGER NOT NULL,
                transaction_hash TEXT NOT NULL,
                transaction_index INTEGER NOT NULL,
                log_index INTEGER NOT NULL,
                removed INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'canonical',
                reorg_id INTEGER,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_chain_contract_block_log_index
                ON chaindexing_events(chain_id, contract_address, block_number, log_index)",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_abi
                ON chaindexing_events(abi)",
            "CREATE INDEX IF NOT EXISTS chaindexing_events_canonical_lookup
                ON chaindexing_events(chain_id, contract_address, block_number, transaction_index, log_index)
                WHERE status = 'canonical'",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_events_identity
                ON chaindexing_events(chain_id, contract_address, block_hash, transaction_hash, log_index)",
            "CREATE TABLE IF NOT EXISTS chaindexing_reorged_blocks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                block_number INTEGER NOT NULL,
                handled_at INTEGER,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE TABLE IF NOT EXISTS chaindexing_blocks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                block_number INTEGER NOT NULL,
                block_hash TEXT NOT NULL,
                parent_hash TEXT NOT NULL,
                block_timestamp INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_blocks_identity
                ON chaindexing_blocks(chain_id, block_number, block_hash)",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_blocks_canonical_number
                ON chaindexing_blocks(chain_id, block_number)
                WHERE status = 'canonical'",
            "CREATE TABLE IF NOT EXISTS chaindexing_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                block_number INTEGER NOT NULL,
                block_hash TEXT NOT NULL,
                block_timestamp INTEGER NOT NULL DEFAULT 0,
                transaction_hash TEXT NOT NULL,
                transaction_index INTEGER NOT NULL,
                from_address TEXT NOT NULL,
                to_address TEXT,
                value TEXT NOT NULL,
                input TEXT NOT NULL,
                raw TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'canonical',
                reorg_id INTEGER,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_transactions_identity
                ON chaindexing_transactions(chain_id, block_hash, transaction_hash)",
            "CREATE INDEX IF NOT EXISTS chaindexing_transactions_canonical_lookup
                ON chaindexing_transactions(chain_id, block_number, transaction_index)
                WHERE status = 'canonical'",
            "CREATE INDEX IF NOT EXISTS chaindexing_transactions_sender_lookup
                ON chaindexing_transactions(chain_id, from_address, block_number)
                WHERE status = 'canonical'",
            "CREATE TABLE IF NOT EXISTS chaindexing_call_traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                block_number INTEGER NOT NULL,
                block_hash TEXT NOT NULL,
                transaction_hash TEXT NOT NULL DEFAULT '',
                trace_index INTEGER NOT NULL,
                trace_address TEXT NOT NULL,
                call_type TEXT,
                from_address TEXT,
                to_address TEXT,
                value TEXT,
                input TEXT,
                output TEXT,
                error TEXT,
                raw TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'canonical',
                reorg_id INTEGER,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_call_traces_identity
                ON chaindexing_call_traces(chain_id, block_hash, transaction_hash, trace_address, trace_index)",
            "CREATE INDEX IF NOT EXISTS chaindexing_call_traces_canonical_lookup
                ON chaindexing_call_traces(chain_id, block_number, transaction_hash, trace_index)
                WHERE status = 'canonical'",
            "CREATE INDEX IF NOT EXISTS chaindexing_call_traces_address_lookup
                ON chaindexing_call_traces(chain_id, from_address, to_address, block_number)
                WHERE status = 'canonical'",
            "CREATE TABLE IF NOT EXISTS chaindexing_block_scans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                block_hash TEXT NOT NULL,
                contract_address TEXT NOT NULL,
                topic_set_hash TEXT NOT NULL,
                log_count INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'canonical',
                reorg_id INTEGER,
                scanned_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_block_scans_identity
                ON chaindexing_block_scans(chain_id, block_hash, contract_address, topic_set_hash)",
            "CREATE INDEX IF NOT EXISTS chaindexing_block_scans_canonical
                ON chaindexing_block_scans(chain_id, contract_address, block_hash)
                WHERE status = 'canonical'",
            "CREATE TABLE IF NOT EXISTS chaindexing_reorgs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                common_ancestor_number INTEGER NOT NULL,
                common_ancestor_hash TEXT NOT NULL,
                fork_block_number INTEGER NOT NULL,
                old_tip_number INTEGER NOT NULL,
                new_tip_number INTEGER NOT NULL,
                depth INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'repairing',
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                repaired_at TEXT
            )",
            "CREATE TABLE IF NOT EXISTS chaindexing_checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain_id INTEGER NOT NULL,
                contract_address TEXT NOT NULL DEFAULT '',
                handler_kind TEXT NOT NULL,
                handler_id TEXT NOT NULL,
                next_block_number INTEGER NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_checkpoints_identity
                ON chaindexing_checkpoints(chain_id, contract_address, handler_kind, handler_id)",
            "CREATE TABLE IF NOT EXISTS chaindexing_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                idempotency_key TEXT NOT NULL,
                chain_id INTEGER NOT NULL,
                contract_address TEXT NOT NULL,
                event_id TEXT NOT NULL,
                source_block_hash TEXT NOT NULL DEFAULT '',
                source_block_number INTEGER NOT NULL DEFAULT 0,
                required_finality TEXT NOT NULL DEFAULT 'latest',
                handler_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                status TEXT NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                next_attempt_at TEXT,
                lease_expires_at TEXT,
                lease_token TEXT,
                inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS chaindexing_outbox_idempotency_key
                ON chaindexing_outbox(idempotency_key)",
            "CREATE INDEX IF NOT EXISTS chaindexing_outbox_status_next_attempt_at
                ON chaindexing_outbox(status, next_attempt_at)",
            "CREATE INDEX IF NOT EXISTS chaindexing_outbox_status_lease_expires_at
                ON chaindexing_outbox(status, lease_expires_at)",
        ]
    }

    pub fn mark_reorged_from_block_statements(
        chain_id: u64,
        fork_block_number: u64,
        reorg_id: i64,
    ) -> Vec<String> {
        let chain_id = chain_id as i64;
        let fork_block_number = fork_block_number as i64;

        vec![
            format!(
                "UPDATE chaindexing_blocks
                 SET status = 'reorged'
                 WHERE chain_id = {chain_id}
                   AND block_number >= {fork_block_number}
                   AND status = 'canonical'"
            ),
            format!(
                "UPDATE chaindexing_events
                 SET status = 'reorged', reorg_id = {reorg_id}
                 WHERE chain_id = {chain_id}
                   AND block_number >= {fork_block_number}
                   AND status = 'canonical'"
            ),
            format!(
                "UPDATE chaindexing_transactions
                 SET status = 'reorged', reorg_id = {reorg_id}
                 WHERE chain_id = {chain_id}
                   AND block_number >= {fork_block_number}
                   AND status = 'canonical'"
            ),
            format!(
                "UPDATE chaindexing_call_traces
                 SET status = 'reorged', reorg_id = {reorg_id}
                 WHERE chain_id = {chain_id}
                   AND block_number >= {fork_block_number}
                   AND status = 'canonical'"
            ),
            format!(
                "UPDATE chaindexing_block_scans
                 SET status = 'reorged', reorg_id = {reorg_id}
                 WHERE chain_id = {chain_id}
                   AND block_hash IN (
                       SELECT block_hash
                       FROM chaindexing_blocks
                       WHERE chain_id = {chain_id}
                         AND block_number >= {fork_block_number}
                   )
                   AND status = 'canonical'"
            ),
        ]
    }

    pub fn lease_pending_outbox_jobs_query(
        limit: u32,
        lease_token: &str,
        lease_seconds: u32,
    ) -> String {
        let limit = limit.max(1);
        let lease_token = sqlite_string_literal(lease_token);

        format!(
            "UPDATE chaindexing_outbox
             SET status = 'dispatching',
                 attempt_count = attempt_count + 1,
                 lease_token = {lease_token},
                 lease_expires_at = datetime('now', '+{lease_seconds} seconds'),
                 updated_at = CURRENT_TIMESTAMP
             WHERE id IN (
                 SELECT id
                 FROM chaindexing_outbox
                 WHERE status = 'pending'
                   AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
                 ORDER BY inserted_at ASC, id ASC
                 LIMIT {limit}
             )
             RETURNING id, idempotency_key, attempt_count, lease_token"
        )
    }
}

fn sqlite_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn migrated_connection() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");

        for migration in SqlitePrototype::internal_migrations() {
            conn.execute_batch(migration).expect("run sqlite prototype migration");
        }

        conn
    }

    #[test]
    fn sqlite_prototype_schema_creates_internal_tables() {
        let conn = migrated_connection();
        let mut stmt = conn
            .prepare(
                "SELECT name
                 FROM sqlite_master
                 WHERE type = 'table'
                   AND name LIKE 'chaindexing_%'
                 ORDER BY name",
            )
            .expect("prepare table query");

        let table_names = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query table names")
            .collect::<Result<Vec<_>, _>>()
            .expect("read table names");

        for expected in [
            "chaindexing_block_scans",
            "chaindexing_blocks",
            "chaindexing_call_traces",
            "chaindexing_checkpoints",
            "chaindexing_contract_addresses",
            "chaindexing_events",
            "chaindexing_nodes",
            "chaindexing_outbox",
            "chaindexing_reorged_blocks",
            "chaindexing_reorgs",
            "chaindexing_root_states",
            "chaindexing_transactions",
        ] {
            assert!(table_names.contains(&expected.to_string()));
        }
    }

    #[test]
    fn sqlite_prototype_rolls_back_internal_batch_atomically() {
        let mut conn = migrated_connection();

        {
            let txn = conn.transaction().expect("start sqlite transaction");
            txn.execute(
                "INSERT INTO chaindexing_blocks
                 (chain_id, block_number, block_hash, parent_hash, block_timestamp, status)
                 VALUES (1, 10, '0xblock', '0xparent', 1, 'canonical')",
                [],
            )
            .expect("insert block");
            txn.execute(
                "INSERT INTO chaindexing_events
                 (id, chain_id, contract_address, contract_name, abi, parameters, topics,
                  block_hash, block_number, block_timestamp, transaction_hash,
                  transaction_index, log_index, removed)
                 VALUES ('event-1', 1, '0xcontract', 'ERC721', 'Transfer', '{}', '[]',
                         '0xblock', 10, 1, '0xtx', 0, 0, 0)",
                [],
            )
            .expect("insert event");
            txn.execute(
                "INSERT INTO chaindexing_transactions
                 (chain_id, block_number, block_hash, transaction_hash, transaction_index,
                  from_address, value, input, raw)
                 VALUES (1, 10, '0xblock', '0xtx', 0, '0xfrom', '0', '0x', '{}')",
                [],
            )
            .expect("insert transaction");

            txn.rollback().expect("rollback sqlite transaction");
        }

        let block_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chaindexing_blocks", [], |row| {
                row.get(0)
            })
            .expect("count blocks");
        let event_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chaindexing_events", [], |row| {
                row.get(0)
            })
            .expect("count events");
        let transaction_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chaindexing_transactions", [], |row| {
                row.get(0)
            })
            .expect("count transactions");

        assert_eq!(block_count, 0);
        assert_eq!(event_count, 0);
        assert_eq!(transaction_count, 0);
    }

    #[test]
    fn sqlite_prototype_preserves_internal_identity_keys() {
        let conn = migrated_connection();

        conn.execute(
            "INSERT INTO chaindexing_events
             (id, chain_id, contract_address, contract_name, abi, parameters, topics,
              block_hash, block_number, block_timestamp, transaction_hash,
              transaction_index, log_index, removed)
             VALUES ('event-1', 1, '0xcontract', 'ERC721', 'Transfer', '{}', '[]',
                     '0xblock', 10, 1, '0xtx', 0, 0, 0)",
            [],
        )
        .expect("insert event");

        let duplicate = conn.execute(
            "INSERT INTO chaindexing_events
             (id, chain_id, contract_address, contract_name, abi, parameters, topics,
              block_hash, block_number, block_timestamp, transaction_hash,
              transaction_index, log_index, removed)
             VALUES ('event-2', 1, '0xcontract', 'ERC721', 'Transfer', '{}', '[]',
                     '0xblock', 10, 1, '0xtx', 0, 0, 0)",
            [],
        );

        assert!(duplicate.is_err());
    }

    #[test]
    fn sqlite_prototype_marks_reorged_internal_rows_consistently() {
        let conn = migrated_connection();

        conn.execute(
            "INSERT INTO chaindexing_blocks
             (chain_id, block_number, block_hash, parent_hash, block_timestamp, status)
             VALUES (1, 10, '0xblock', '0xparent', 1, 'canonical')",
            [],
        )
        .expect("insert block");
        conn.execute(
            "INSERT INTO chaindexing_events
             (id, chain_id, contract_address, contract_name, abi, parameters, topics,
              block_hash, block_number, block_timestamp, transaction_hash,
              transaction_index, log_index, removed)
             VALUES ('event-1', 1, '0xcontract', 'ERC721', 'Transfer', '{}', '[]',
                     '0xblock', 10, 1, '0xtx', 0, 0, 0)",
            [],
        )
        .expect("insert event");
        conn.execute(
            "INSERT INTO chaindexing_transactions
             (chain_id, block_number, block_hash, transaction_hash, transaction_index,
              from_address, value, input, raw)
             VALUES (1, 10, '0xblock', '0xtx', 0, '0xfrom', '0', '0x', '{}')",
            [],
        )
        .expect("insert transaction");
        conn.execute(
            "INSERT INTO chaindexing_call_traces
             (chain_id, block_number, block_hash, transaction_hash, trace_index,
              trace_address, raw)
             VALUES (1, 10, '0xblock', '0xtx', 0, '0', '{}')",
            [],
        )
        .expect("insert trace");
        conn.execute(
            "INSERT INTO chaindexing_block_scans
             (chain_id, block_hash, contract_address, topic_set_hash, log_count)
             VALUES (1, '0xblock', '0xcontract', 'topics', 1)",
            [],
        )
        .expect("insert scan");

        for statement in SqlitePrototype::mark_reorged_from_block_statements(1, 10, 99) {
            conn.execute_batch(&statement).expect("mark row reorged");
        }

        for table in [
            "chaindexing_blocks",
            "chaindexing_events",
            "chaindexing_transactions",
            "chaindexing_call_traces",
            "chaindexing_block_scans",
        ] {
            let status: String = conn
                .query_row(&format!("SELECT status FROM {table} LIMIT 1"), [], |row| {
                    row.get(0)
                })
                .expect("load status");
            assert_eq!(status, "reorged");
        }
    }

    #[test]
    fn sqlite_prototype_leases_pending_outbox_jobs_once() {
        let conn = migrated_connection();

        for id in 1..=3 {
            conn.execute(
                "INSERT INTO chaindexing_outbox
                 (idempotency_key, chain_id, contract_address, event_id, handler_id, payload, status)
                 VALUES (?1, 1, '0xcontract', ?2, 'handler', '{}', 'pending')",
                params![format!("key-{id}"), format!("event-{id}")],
            )
            .expect("insert pending outbox job");
        }

        let first_query = SqlitePrototype::lease_pending_outbox_jobs_query(2, "lease-1", 30);
        let first_count = conn
            .prepare(&first_query)
            .expect("prepare first lease")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("lease first jobs")
            .count();

        let second_query = SqlitePrototype::lease_pending_outbox_jobs_query(2, "lease-2", 30);
        let second_count = conn
            .prepare(&second_query)
            .expect("prepare second lease")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("lease second jobs")
            .count();

        assert_eq!(first_count, 2);
        assert_eq!(second_count, 1);

        let dispatching_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chaindexing_outbox WHERE status = 'dispatching'",
                [],
                |row| row.get(0),
            )
            .expect("count dispatching jobs");

        assert_eq!(dispatching_count, 3);
    }
}
