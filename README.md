# Chaindexing

[<img alt="github" src="https://img.shields.io/badge/Github-chaindexing%2Fchaindexing--rs-blue?logo=github" height="20">](https://github.com/chaindexing/chaindexing-rs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/chaindexing.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/chaindexing)
[<img alt="chaindexing build" src="https://img.shields.io/github/actions/workflow/status/chaindexing/chaindexing-rs/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/chaindexing/chaindexing-rs/actions?query=branch%3Amain)

Embed an EVM event indexer in Rust and materialize reorg-aware contract state into Postgres.

Chaindexing is for teams that want indexed blockchain data in a database they own, with ordinary
SQL read paths, deterministic Rust handlers, bounded reorg repair, and production-facing controls
for RPC pressure, finality, and external side effects.

[Quickstart](#quickstart) | [Why Chaindexing](#why-chaindexing) | [Guarantees](#guarantees) | [Reorg Handling](docs/reorg-handling.md) | [Finality Policies](docs/finality-policies.md) | [Examples](https://github.com/chaindexing/chaindexing-examples/tree/main/rust) | [Production Limits](#production-limits) | [Roadmap](#roadmap) | [Contributing](#contributing)

## Quickstart

Add Chaindexing to a Tokio Rust service:

```toml
[dependencies]
chaindexing = "0.1.81"
tokio = { version = "1", features = ["full"] }
```

Chaindexing currently requires Rust 1.91 or newer.

### Ethereum types

Chaindexing uses Alloy for Ethereum primitives, ABI parsing, and JSON-RPC provider types. Most
applications can stay on Chaindexing's public API and use the reexported `Address`, `I256`, and
`U256` types from `chaindexing` or `chaindexing::prelude`.

Applications that implement a custom `IngesterProvider` should use Alloy RPC types in the trait
methods, including `alloy::rpc::types::{Block, Filter, Log}`. `ProviderError::CustomError(String)`
remains available for custom provider failures.

A minimal NFT ownership indexer has three pieces: a Postgres state table, a deterministic event
handler, and an indexer runtime.

Define the state you want to query from Postgres:

```rust
use chaindexing::augmenting_std::serde::{Deserialize, Serialize};
use chaindexing::state_migrations;
use chaindexing::states::{ContractState, StateMigrations};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "chaindexing::augmenting_std::serde")]
pub struct Nft {
    pub token_id: u32,
    pub owner_address: String,
}

impl ContractState for Nft {
    fn table_name() -> &'static str {
        "nfts"
    }
}

pub struct NftMigrations;

impl StateMigrations for NftMigrations {
    fn migrations(&self) -> &'static [&'static str] {
        state_migrations!([r#"
            CREATE TABLE IF NOT EXISTS nfts (
                token_id INTEGER NOT NULL,
                owner_address TEXT NOT NULL
            )
        "#])
    }
}
```

Handle the contract event that changes that state:

```rust
use chaindexing::states::{ContractState, Filters, Updates};
use chaindexing::{EventContext, EventHandler};

use crate::states::Nft;

pub struct TransferHandler;

#[chaindexing::augmenting_std::async_trait]
impl EventHandler for TransferHandler {
    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }
    async fn handle_event<'a, 'b>(&self, context: EventContext<'a, 'b>) {
        let event_params = context.get_event_params();

        let _from = event_params.get_address_string("from");
        let to = event_params.get_address_string("to");
        let token_id = event_params.get_u32("tokenId");

        if let Some(existing_nft) =
            Nft::read_one(&Filters::new("token_id", token_id), &context).await
        {
            let updates = Updates::new("owner_address", &to);
            existing_nft.update(&updates, &context).await;
        } else {
            let new_nft = Nft {
                token_id,
                owner_address: to,
            };

            new_nft.create(&context).await;
        }
    }
}
```

Run the indexer against your Postgres database and JSON-RPC provider:

```rust
use chaindexing::{Chain, ChainId, Contract, Indexer, ReorgMode, RuntimeConfig};

async fn start_indexer() -> Result<(), chaindexing::ChaindexingError> {
    let erc721 = Contract::new("ERC721")
        .add_event_handler(TransferHandler)
        .add_state_migrations(NftMigrations)
        .add_address(
            "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
            &ChainId::Mainnet,
            17_773_490,
        );

    Indexer::new(&std::env::var("DATABASE_URL").unwrap())
        .chain(Chain::mainnet(&std::env::var("MAINNET_JSON_RPC_URL").unwrap()))
        .contract(erc721)
        .runtime(RuntimeConfig::realtime())
        .reorg_mode(ReorgMode::Balanced)
        .run()
        .await
}
```

`run()` owns the indexer lifecycle and blocks until Ctrl-C before shutting workers down. Services
or tests that need to manage shutdown themselves should call `Indexer::start()` and keep the
returned `IndexingHandle`.

Then query the indexed state with ordinary SQL:

```sql
SELECT token_id, owner_address, chain_id, contract_address
FROM nfts
WHERE token_id = 42;
```

Full working examples live in
[chaindexing-examples/rust](https://github.com/chaindexing/chaindexing-examples/tree/main/rust).

### Postgres TLS

For managed Postgres, set `sslmode=require` or `sslmode=verify-full` in `DATABASE_URL`.
Chaindexing uses the same TLS policy for Diesel pool connections and raw `tokio-postgres`
connections, and verifies server certificates with platform roots plus any custom CA bundle you
provide. Standard `sslrootcert=/path/to/ca.pem` connection parameters are supported for CA
bundles. A missing `sslmode` stays non-TLS for local development compatibility.

Private CA bundles can be supplied explicitly:

```rust
use chaindexing::{Indexer, PostgresTlsConfig};

let database_url = std::env::var("DATABASE_URL").unwrap();
let ca_bundle = std::fs::read("postgres-ca.pem").unwrap();

Indexer::new_with_tls(
    &database_url,
    PostgresTlsConfig::require().with_ca_cert_pem(ca_bundle),
);
```

## Why Chaindexing

| Serious indexing concern | Chaindexing's answer |
| --- | --- |
| Own the data model | Indexed state is materialized into your Postgres tables, so app reads can use SQL, views, BI tools, ORMs, exports, and normal database operations. |
| Reorg correctness | Blocks are tracked by hash and parent hash; canonical events, state versions, scans, and handler cursors are rewound inside the configured finality window. |
| External side effects | Direct side-effect handlers exist for compatibility, while durable webhooks, notifications, queues, and bridge jobs should go through the Postgres outbox. |
| Runtime control | Workload profiles expose batch size, worker limits, RPC in-flight budgets, retry/backoff, and polling cadence without YAML or a separate indexing service. |
| Dynamic contracts | Handlers can include newly discovered contract addresses at runtime, useful for factory patterns such as Uniswap pools. |
| Tail-aware handlers | `EventContext::is_at_block_tail()` and `SideEffectContext::is_at_block_tail()` let handlers distinguish historical catch-up from the latest block already ingested for that contract address. |

## Guarantees

Chaindexing's Postgres backend is designed around these guarantees:

- Event ingestion is idempotent for the canonical event identity: `chain_id`, `contract_address`, `block_hash`, `transaction_hash`, and `log_index`.
- Canonical block tracking uses `block_hash` and `parent_hash`; replaced blocks, scans, and events are marked `reorged`.
- Event logs are fetched by `blockHash` when the provider supports it, with durable empty-scan records in `chaindexing_block_scans`.
- Handler state is deterministic and replayable from persisted events.
- Reorg repair is bounded by the configured finality/confirmation policy and records repairs in `chaindexing_reorgs`.
- Ingestion and handler checkpoints are stored durably in Postgres and written transactionally with cursor updates.
- Multi-node leader election uses a Postgres advisory lock by default.
- Empty event batches are safe to retry.
- Direct side-effect handlers are supported for compatibility; durable external side effects should be written to `chaindexing_outbox` with `SideEffectContext::enqueue_outbox`, `enqueue_outbox_with_configured_finality`, or `enqueue_outbox_with_finality`.

Non-goals:

- Chaindexing does not promise reorg safety beyond the configured confirmation window.
- Chaindexing does not promise exactly-once network calls from direct side-effect handlers.
- Postgres is the supported production backend while the core guarantees are being completed.

## Reorg and finality presets

Use presets to express product behavior without configuring the reorg algorithm:

```rust
use chaindexing::{Indexer, IndexingFinality, ReorgMode, SideEffectFinality};

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Realtime); // low-latency UI/feed use cases

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Balanced); // analytics/reporting default for production

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::FinalityFirst); // payments, claims, settlement

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Balanced)
    .indexing_finality(IndexingFinality::LatestWithConfirmations(12))
    .side_effect_finality(SideEffectFinality::Finalized);
```

See [Reorg Handling](docs/reorg-handling.md), [Finality Policies](docs/finality-policies.md),
and [Side Effects and Reorgs](docs/side-effects-and-reorgs.md) for the operational model.

## Runtime profiles and limits

Use runtime profiles to describe workload behavior, then override concrete resource limits only
when you know your database or RPC budget. Presets are not named after environments because a
library cannot reliably know whether a laptop, CI runner, staging box, or production node is
resource-constrained.

```rust
use chaindexing::{Indexer, RpcPolicy, RuntimeConfig, RuntimeLimits};

// Low-latency indexing for app feeds and dashboards.
Indexer::new(&database_url)
    .runtime(RuntimeConfig::realtime());

// Historical catch-up with explicit resource/RPC limits.
Indexer::new(&database_url)
    .runtime(
        RuntimeConfig::backfill()
            .limits(
                RuntimeLimits::throughput()
                    .db_connections(16)
                    .max_ingester_workers(8)
                    .max_handler_workers(8),
            )
            .rpc(RpcPolicy::throughput().max_in_flight(64).max_per_chain(16))
    );

// Cheap or rate-limited providers.
Indexer::new(&database_url)
    .runtime(
        RuntimeConfig::rpc_constrained()
            .rpc(RpcPolicy::limited().max_in_flight(4).max_per_chain(2).requests_per_second(5)),
    );

// Deterministic single-worker behavior for tests and reproducible debugging.
Indexer::new(&database_url)
    .runtime(RuntimeConfig::deterministic());

// Existing leader election still controls multi-process ownership. Runtime profiles
// tune the active node's work budget instead of guessing from environment names.
```

Important runtime knobs:

| Setting | What it controls |
| --- | --- |
| `db_connections` | Postgres pool size for pooled ingestion/supervisor work; handler raw clients are bounded by handler workers. |
| `max_ingester_workers` | Maximum ingestion worker count; capped by available chains. |
| `max_handler_workers` | Maximum handler worker count; effective parallelism still respects state ordering. |
| `max_in_flight` | Global JSON-RPC in-flight request budget; also caps effective ingestion workers. |
| `max_per_chain` | Per-chain JSON-RPC in-flight cap, preventing one chain from starving others. |
| `requests_per_second` | Optional provider rate-limit hint for RPC-constrained workloads. |
| `retry_attempts`, `base_backoff_ms`, `max_backoff_ms` | Provider retry and capped exponential backoff policy. |
| `blocks_per_batch` | Block-range size for ingestion and handler batches. |

See [Runtime Profiles](docs/runtime-profiles.md) for profile selection, policy axes, and ordering
guarantees.

Compatibility setters like `.blocks_per_batch(...)`, `.ingestion_rate_ms(...)`,
`.handler_rate_ms(...)`, and `.chain_concurrency(...)` still work. New applications should prefer
`.runtime(...)` because it keeps workload, resource limits, RPC policy, and polling cadence explicit.

## Durable outbox

Example side-effect outbox usage:

```rust
use chaindexing::{SideEffectContext, SideEffectHandler};

pub struct TransferSideEffectHandler;

#[chaindexing::augmenting_std::async_trait]
impl SideEffectHandler for TransferSideEffectHandler {
    type SharedState = ();

    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }

    async fn handle_event<'a>(&self, context: SideEffectContext<'a, Self::SharedState>) {
        let token_id = context.get_event_params().get_u32("tokenId");

        context
            .enqueue_outbox("nft-transfer-notification", &format!("token {token_id} moved"))
            .await;

        // For workflows that should honor the indexer's side-effect finality policy:
        context
            .enqueue_outbox_with_configured_finality(
                "nft-transfer-webhook",
                &format!("token {token_id} moved"),
            )
            .await;
    }
}
```

Dispatch pending outbox jobs from a worker process:

```rust
use chaindexing::{
    dispatch_pending_outbox_jobs, OutboxDispatchConfig, OutboxDispatcher,
    OutboxFinalityWatermark, OutboxJob,
};

struct Dispatcher;

#[chaindexing::augmenting_std::async_trait]
impl OutboxDispatcher for Dispatcher {
    async fn dispatch(&self, job: OutboxJob) -> Result<(), String> {
        // Send job.payload to your webhook, queue, bridge, or notification provider.
        Ok(())
    }
}

async fn dispatch_outbox(
    latest_block_number: u64,
    safe_block_number: u64,
    finalized_block_number: u64,
) -> usize {
    dispatch_pending_outbox_jobs(
        &std::env::var("DATABASE_URL").unwrap(),
        &Dispatcher,
        OutboxDispatchConfig::default().with_finality_watermark(OutboxFinalityWatermark {
            chain_id: 1,
            latest_block_number: Some(latest_block_number),
            safe_block_number: Some(safe_block_number),
            finalized_block_number: Some(finalized_block_number),
        }),
    )
    .await
}
```

Outbox dispatch is intentionally at-least-once. Dispatchers should make external calls idempotent
using `job.idempotency_key`. Chaindexing leases jobs before dispatch, retries expired leases, and
counts every lease start toward `max_attempts` so process crashes cannot retry poison jobs forever.

Application code can read indexed state directly from Postgres:

```rust
use chaindexing::states::{ContractState, Filters};
use chaindexing::ChainId;

async fn read_nft() -> Option<Nft> {
    Nft::read_one_from_postgres(
        &std::env::var("DATABASE_URL").unwrap(),
        &ChainId::Mainnet,
        "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
        &Filters::new("token_id", 42),
    )
    .await
}
```

## Capabilities

| Capability | Status |
| --- | --- |
| EVM log indexing | Supported for any EVM chain available through an HTTP JSON-RPC provider. |
| Postgres state materialization | Supported. Chaindexing owns internal tables and writes your declared state tables. |
| Postgres TLS | Supported for pooled and raw clients through rustls; use `sslmode=require` or `sslmode=verify-full`, with custom CAs through `sslrootcert` or `PostgresTlsConfig`. |
| Multi-chain indexing | Supported through multiple `Chain` configs. |
| Runtime-discovered contracts | Supported with `chaindexing::include_contract(...)`. |
| Handler tail heuristic | Supported with `is_at_block_tail()` on pure and side-effect handler contexts. |
| Reorg repair | Supported inside the configured confirmation/finality window. |
| Durable side-effect dispatch | Supported through `chaindexing_outbox`; dispatch is intentionally at-least-once. |
| Raw transactions and traces | Not supported yet. |
| SQLite or non-Postgres backends | Not supported yet; Postgres is the production backend. |

## Production Limits

Chaindexing is still young and optimized for Rust ergonomics plus Postgres ownership. The runtime
profile API makes the main scaling tradeoffs explicit, but production deployments should account
for these limits:

- **Historical throughput:** `RuntimeConfig::backfill()` increases worker, RPC, and batch defaults for catch-up. Override `blocks_per_batch`, `max_ingester_workers`, and `max_in_flight` based on your provider and Postgres capacity.
- **Worker caps:** `max_ingester_workers` and `max_handler_workers` are caps, not promises. They are capped by available chains and by state-ordering partitions.
- **Handler ordering:** `ContractState` and `ChainState` handlers stay ordered within their logical partition. More handler workers help independent chains/contracts, but not a single hot ordered partition.
- **Handler batch shape:** Handler loading is block-bounded, not event-bounded. A batch includes every matching event in the selected blocks so cursors never skip logs inside a block. Lower `blocks_per_batch` to reduce multi-block batches; a single extremely hot block still has to fit in memory and one handler transaction.
- **Database bottlenecks:** `db_connections` bounds the pooled Postgres work used by ingestion and supervision, while handler raw clients scale with `max_handler_workers`. More connections only help if Postgres has spare CPU, IO, and lock capacity.
- **RPC provider limits:** `max_in_flight`, `max_per_chain`, and optional `requests_per_second` express provider pressure. Public endpoints often cap block ranges and requests per second.
- **Deep backfills:** Indexing hundreds of millions of historical blocks has not been fully optimized. Prefer `RuntimeConfig::backfill()` with explicit limits, or start closer to the present block.
- **Error reporting:** Some internal database and provider failures are still surfaced as opaque runtime errors. Improving typed errors and diagnostics is active roadmap work.

## Roadmap

- Support raw transaction and call trace indexing.
- Improve error handling, messages, and reporting.
- Add a minimal UI for inspecting events and indexed states.
- Evaluate SQLite or other local backends after the Postgres guarantees are complete.

## Contributing

All contributions are welcome. Before working on a PR, please consider opening an issue detailing the feature/bug. Equally, when submitting a PR, please ensure that all checks pass to facilitate a smooth review process.

Postgres-backed tests require `TEST_DATABASE_URL`. Local runs skip those tests when the variable is
absent. CI fails on a missing `TEST_DATABASE_URL` unless `ALLOW_DB_TEST_SKIP=1` is set explicitly.
