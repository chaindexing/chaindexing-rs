# Indexed Transactions And Call Traces

Chaindexing indexes logs and derived state by default. Full transaction payloads and call traces are
available as opt-in internal tables when an application needs lower-level inspection data next to
its event index.

## Enabling Indexed Data

```rust
use chaindexing::{Chain, Contract, Indexer};

let indexer = Indexer::new(&std::env::var("DATABASE_URL").unwrap())
    .chain(Chain::mainnet(&std::env::var("ETH_RPC_URL").unwrap()))
    .contract(Contract::new("ERC721"))
    .raw_transactions()
    .call_traces();
```

The equivalent advanced config methods are `Config::with_raw_transaction_indexing()`,
`Config::with_call_trace_indexing()`, and `Config::with_indexed_data(...)`.

## Postgres Tables

`chaindexing_transactions` stores canonical full JSON-RPC transaction payloads:

- block identity: `chain_id`, `block_number`, `block_hash`, `block_timestamp`
- transaction identity: `transaction_hash`, `transaction_index`
- common lookup fields: `from_address`, `to_address`, `value`, `input`
- raw payload: `raw JSONB`
- reorg lifecycle: `status`, `reorg_id`

`chaindexing_call_traces` stores flattened call trace payloads:

- block identity: `chain_id`, `block_number`, `block_hash`
- trace identity: `transaction_hash`, `trace_address`, `trace_index`
- common lookup fields: `call_type`, `from_address`, `to_address`, `value`, `input`, `output`,
  `error`
- raw payload: `raw JSONB`
- reorg lifecycle: `status`, `reorg_id`

Both tables use `status = 'canonical'` for current-chain data. When Chaindexing detects a canonical
block mismatch, replaced rows are marked `reorged` and replacement rows are written for the repaired
canonical blocks.

## Provider Requirements

Raw transaction indexing fetches full blocks with transaction objects. Alloy-backed providers use
`eth_getBlockByNumber(..., true)` through Alloy's full block API.

Call trace indexing uses `debug_traceBlockByHash` with Geth's `callTracer` by default, then flattens
the returned root calls and nested `calls` arrays into stable trace-address rows. Many hosted RPC
providers disable debug tracing or expose trace APIs with provider-specific limits. Custom providers
can implement `IngesterProvider::get_call_traces_by_block_hash(...)` to adapt other trace methods
while keeping the same storage path. Trace requests use the configured RPC retry, concurrency, and
rate-limit policy.

## Inspection Queries

Use `InspectionQueries` with `ChaindexingRepo::load_data_list(...)` when you need read-only
inspection without hand-writing common table scans:

```rust
use chaindexing::{ChaindexingRepo, InspectionQueries, InspectionQuery, LoadsDataWithRawQuery};

async fn load_recent_transactions(
    repo_client: &tokio_postgres::Client,
) -> Result<Vec<serde_json::Value>, chaindexing::RepoError> {
    let query = InspectionQuery::new(1).from_block_number(18_000_000).limit(100);

    ChaindexingRepo::load_data_list(
        repo_client,
        &InspectionQueries::canonical_transactions(query),
    )
    .await
}
```

These helpers are intentionally query builders rather than a server. They keep the core library
Postgres-first while leaving room for thin local or hosted inspection surfaces.

## Packaged Inspection UI

The crate also ships a minimal read-only UI over the same inspection queries:

```sh
DATABASE_URL=postgres://user:pass@localhost:5432/app \
  cargo run -p chaindexing --bin chaindexing-inspect
```

By default it listens on `127.0.0.1:8787`. Set `CHAINDEXING_INSPECT_ADDR` to bind a different local
address. The UI exposes only `GET` routes and uses the status-scoped canonical queries above.
