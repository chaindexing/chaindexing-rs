# Local Backend Evaluation

Postgres remains Chaindexing's production backend. The internal model now depends on guarantees that
SQLite and embedded engines must match before they become supported targets.

## Required Guarantees

A local backend must preserve these behaviors:

- transactional ingestion that atomically writes blocks, block scans, events, transactions, traces,
  and checkpoints
- canonical/reorged status transitions for every internal table touched by ingestion
- idempotent inserts for event, block, transaction, trace, checkpoint, and outbox identity keys
- ordered handler reads that remain block-bounded rather than event-bounded
- state versioning and state view migrations compatible with existing `ContractState` and
  `ChainState` APIs
- outbox leasing semantics that do not double-dispatch jobs under normal process concurrency

## Current SQLite Prototype

SQLite is the first local backend under evaluation because it has broad deployment support and
transactional semantics. The crate now exposes `SqlitePrototype` as an explicit, unsupported
prototype surface for exercising local-backend guarantees without changing the production backend.

The prototype includes:

- SQLite-compatible internal migrations for Chaindexing's root, node, contract-address, event,
  block, block-scan, transaction, call-trace, checkpoint, reorg, and outbox tables
- reorg marking statements that keep canonical/reorged status transitions aligned across internal
  tables
- a single-writer outbox leasing query that models the Postgres lease lifecycle
- a guarantee report through `SqlitePrototype::report()` so callers and docs can distinguish
  proven prototype behavior from unsupported production behavior

```rust
use chaindexing::SqlitePrototype;

let report = SqlitePrototype::report();
let migrations = SqlitePrototype::internal_migrations();
```

## Evaluation Shape

The prototype should prove:

- a fresh install creates all internal tables
- ingestion writes logs, transactions, traces, block scans, and checkpoints in one transaction
- reorg repair marks every affected internal table consistently
- handler state updates remain ordered by chain, contract, and state partition
- outbox jobs lease and retry safely under at least two concurrent worker tasks
- existing examples can run without code changes except the repo constructor

## Non-Goals For Now

The current code should not claim SQLite support until the prototype passes the same behavioral
tests as Postgres. Query-builder compatibility alone is not enough because Chaindexing relies on
isolation, conflict handling, JSON storage, and leasing behavior that differ across SQL engines.
