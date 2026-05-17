# Chaindexing

[<img alt="github" src="https://img.shields.io/badge/Github-jurshsmith%2Fchaindexing-blue?logo=github" height="20">](https://github.com/jurshsmith/chaindexing-rs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/chaindexing.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/chaindexing)
[<img alt="diesel-streamer build" src="https://img.shields.io/github/actions/workflow/status/jurshsmith/chaindexing-rs/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/jurshsmith/chaindexing-rs/actions?query=branch%3Amain)

Index any EVM chain and query in SQL

[Getting Started](#getting-started) | [Examples](https://github.com/chaindexing/chaindexing-examples/tree/main/rust) | [Design Goals & Features](#design-goals--features) | [RoadMap](#roadmap) | [Contributing](#contributing)

## Getting Started

📊 Here is what indexing and tracking owers of your favorite NFTs looks like:

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

A quick and effective way to get started is by exploring the comprehensive examples provided here: [https://github.com/chaindexing/chaindexing-examples/tree/main/rust](https://github.com/chaindexing/chaindexing-examples/tree/main/rust).

Minimal runtime setup now looks like this:

```rust
use chaindexing::{Chain, ChainId, Contract, Indexer};

# async fn start() -> Result<(), chaindexing::ChaindexingError> {
let erc721 = Contract::new("ERC721")
    .add_event_handler(TransferHandler)
    .add_address(
        "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
        &ChainId::Mainnet,
        17_773_490,
    );

Indexer::new(&std::env::var("DATABASE_URL").unwrap())
    .chain(Chain::mainnet(&std::env::var("MAINNET_JSON_RPC_URL").unwrap()))
    .contract(erc721)
    .run()
    .await?;
# Ok(())
# }
```

`run()` owns the indexer lifecycle and blocks until Ctrl-C before shutting workers down. Services
or tests that need to manage shutdown themselves should call `Indexer::start()` and keep the
returned `IndexingHandle`.

## Guarantees

Chaindexing's Postgres backend is being hardened around these guarantees:

- Event ingestion is idempotent for the canonical event identity: `chain_id`, `contract_address`, `block_hash`, `transaction_hash`, and `log_index`.
- Handler state is deterministic and replayable from persisted events.
- Reorg repair is bounded by the configured confirmation depth and now records canonical block hashes for detected event-bearing blocks.
- Ingestion and handler checkpoints are stored durably in Postgres and written transactionally with cursor updates.
- Multi-node leader election uses a Postgres advisory lock by default.
- Empty event batches are safe to retry.
- Direct side-effect handlers are supported for compatibility; durable external side effects should be written to `chaindexing_outbox` with `SideEffectContext::enqueue_outbox`.

Non-goals:

- Chaindexing does not promise reorg safety beyond the configured confirmation window.
- Chaindexing does not promise exactly-once network calls from direct side-effect handlers.
- Postgres is the supported production backend while the core guarantees are being completed.

Example side-effect outbox usage:

```rust
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
    }
}
```

Dispatch pending outbox jobs from a worker process:

```rust
use chaindexing::{dispatch_pending_outbox_jobs, OutboxDispatchConfig, OutboxDispatcher, OutboxJob};

struct Dispatcher;

#[chaindexing::augmenting_std::async_trait]
impl OutboxDispatcher for Dispatcher {
    async fn dispatch(&self, job: OutboxJob) -> Result<(), String> {
        // Send job.payload to your webhook, queue, bridge, or notification provider.
        Ok(())
    }
}

# async fn dispatch() {
let dispatched = dispatch_pending_outbox_jobs(
    &std::env::var("DATABASE_URL").unwrap(),
    &Dispatcher,
    OutboxDispatchConfig::default(),
)
.await;
# }
```

Outbox dispatch is intentionally at-least-once. Dispatchers should make external calls idempotent
using `job.idempotency_key`. Chaindexing leases jobs before dispatch, retries expired leases, and
counts every lease start toward `max_attempts` so process crashes cannot retry poison jobs forever.

Application code can read indexed state directly from Postgres:

```rust
use chaindexing::states::{ContractState, Filters};
use chaindexing::ChainId;

# async fn read_nft() {
let nft = Nft::read_one_from_postgres(
    &std::env::var("DATABASE_URL").unwrap(),
    &ChainId::Mainnet,
    "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
    &Filters::new("token_id", 42),
)
.await;
# }
```

## Design Goals & Features

- 💸&nbsp;Free forever<br/>
- ⚡&nbsp;Real-time use-cases<br/>
- 🌐&nbsp;Multi-chain<br/>
- 🧂&nbsp;Granular, 🧩 Modular & 📈 Scalable<br/>
- 🌍&nbsp;Environment-agnostic to allow inspecting 🔍 & replicating indexes anywhere!<br/>
- 🔓&nbsp;ORM-agnostic, use any ORM to access indexed data<br/>
- 📤&nbsp;Easy export to any data lake: S3, Snowflake, etc.<br/>
- 🚫&nbsp;No complex YAML/JSON/CLI config<br/>
- 💪&nbsp;Index contracts discovered at runtime<br/>
- ✨&nbsp;Handles re-org with no UX impact<br/>
- 🔥&nbsp;Side effect handling for notifications & bridging use cases<br/>
- 💸&nbsp;Optimize RPC cost by indexing when certain activities happen in your DApp<br/>
- 💎&nbsp;Language-agnostic, so no macros!<br/>

## RoadMap

- ⬜&nbsp;Expose `is_at_block_tail` flag to improve op heuristics for applications<br/>
- ⬜&nbsp;Support SQLite Database (Currently supports only Postgres)<br/>
- ⬜&nbsp;Support indexing raw transactions & call traces.<br/>
- ⬜&nbsp;Improved error handling/messages/reporting (Please feel free to open an issue when an opaque runtime error is encountered)<br/>
- ⬜&nbsp;Support TLS connections<br/>
- ⬜&nbsp;Minimal UI for inspecting events and indexed states<br/>

## Performance Considerations & Limitations

Chaindexing is still young and optimized for ergonomics rather than raw throughput. The default configuration works well for real-time indexing of a few contracts, but historical backfills or very high-volume workloads may expose the following constraints:

- 🐢 **Historical Throughput:** Each chain ingester pulls **`blocks_per_batch`** blocks every **`ingestion_rate_ms`** milliseconds. With the defaults (450 blocks / 20 000 ms) this translates to roughly **22 blocks / s per chain**. Tune these knobs to trade throughput for RPC cost.
- 🔗 **Chain Concurrency:** Only `chain_concurrency` chains are ingested in parallel (default **4**). Additional chains are processed sequentially.
- ⚙️ **Handler Cadence:** Event handlers execute every `handler_rate_ms` (default **4 000 ms**). If a contract emits thousands of events per block this cycle can lag behind ingestion.
- 📦 **Handler Batch Shape:** Handler loading is block-bounded, not event-bounded. A batch includes every matching event in the selected blocks so cursors never skip logs inside a block. Lower `blocks_per_batch` to reduce multi-block batches; a single extremely hot block still has to fit in memory and one handler transaction.
- 🗄️ **Database Bottlenecks:** Chaindexing currently supports **Postgres** only. Inserts are batched inside transactions over a limited connection pool—disk or network latency can throttle the pipeline.
- 🌐 **RPC Provider Limits:** Latency and rate-limits of your JSON-RPC provider (e.g. Alchemy, Infura) directly affect indexing speed. Public endpoints often cap block ranges and requests per second.
- ⏳ **Deep Backfills:** Indexing hundreds of millions of historical blocks has not been fully optimized and may require substantial time and memory. Consider chunked backfills or starting closer to the present block.

These limitations are passively being addressed; community benchmarks and pull requests are highly appreciated!

## Contributing

All contributions are welcome. Before working on a PR, please consider opening an issue detailing the feature/bug. Equally, when submitting a PR, please ensure that all checks pass to facilitate a smooth review process.

Postgres-backed tests require `TEST_DATABASE_URL`. Local runs skip those tests when the variable is
absent. CI fails on a missing `TEST_DATABASE_URL` unless `ALLOW_DB_TEST_SKIP=1` is set explicitly.
