# Chaindexing Rust Improvement Plan

Execution target: Saturday, May 16, 2026.

This plan is for making `chaindexing` drastically easier and safer for Rust apps that want to index EVM chains into Postgres. The north-star promise is:

> Give Chaindexing Postgres, RPC URLs, contracts, and Rust handlers; get resumable, idempotent, reorg-aware indexing with deterministic SQL state and production-grade operational visibility.

## Guiding Principles

- Preserve the current ergonomic handler model where possible.
- Make correctness explicit before adding new API surface.
- Treat Postgres as the primary product backend, not a generic interchangeable detail.
- Use the examples repository as an acceptance suite, not only as documentation.
- Prefer incremental migrations that keep existing users upgradeable.
- Do not promise exactly-once side effects directly; implement a durable outbox with idempotency.

## External Acceptance Suite

Use `https://github.com/chaindexing/chaindexing-examples` as a compatibility and documentation target.

Saturday setup:

1. Clone `chaindexing-examples` next to this repo.
2. Point the Rust examples at the local `chaindexing` crate via path dependency.
3. Run the Rust NFT and Uniswap examples against the local crate.
4. After each major slice below, rerun the examples or update them intentionally.

Expected example coverage:

- `rust/nfts`: ERC721 transfer indexing.
- `rust/uniswap`: Uniswap V3 pool/swap indexing.
- New examples to add later: dynamic contract discovery, reorg simulation, side-effect outbox, historical backfill.

## Phase 1: State The Contract

Goal: turn implicit claims into testable guarantees.

Tasks:

- Document library guarantees in README and crate docs:
  - event ingestion is idempotent
  - reducer/state handling is deterministic and replayable
  - reorg repair is guaranteed up to configured confirmation depth
  - checkpoints are durable and resumable
  - side effects are durable and idempotent through an outbox
- Document non-goals:
  - no direct exactly-once network side effects
  - no guarantee beyond configured reorg depth
  - no support for non-Postgres backends until Postgres is solid

Acceptance:

- README has a "Guarantees" section.
- Each guarantee maps to either an existing test or a planned test in this file.

## Phase 2: Foundational Correctness Fixes

Goal: fix known correctness issues before reshaping the architecture.

Tasks:

- Fix event ABI discovery so side-effect-only handlers are included.
- Fix `ChainState` lookup so chain-scoped state actually filters by `chain_id`.
- Strengthen event equality/identity to include:
  - `chain_id`
  - `contract_address`
  - `block_hash`
  - `transaction_hash`
  - `log_index`
  - `abi`
- Make empty event batches safe so inserts do not panic on no logs.
- Add uniqueness constraints for stored events.
- Start replacing raw SQL string formatting in state filters/writes with parameterized paths or controlled escaping.

Acceptance:

- Unit tests cover side-effect-only ingestion.
- Unit tests cover same logical `ChainState` key on two different chains.
- Reorg diff tests cover multiple same-ABI events in the same block.
- `cargo test -p chaindexing --lib` passes.
- Existing Rust examples still compile against local crate.

## Phase 3: Event Idempotency

Goal: make ingestion safe to retry.

Tasks:

- Add a canonical event identity:
  - `chain_id`
  - `contract_address`
  - `block_hash`
  - `transaction_hash`
  - `log_index`
- Add a unique index over that identity.
- Change event insertion to `ON CONFLICT DO NOTHING`.
- Ensure handler ordering remains by `block_number`, `transaction_index`, `log_index`.
- Add migration compatibility for existing event tables.

Acceptance:

- Re-running the same ingestion range does not duplicate events.
- Tests prove retrying an ingestion transaction is safe.
- Examples can be restarted without duplicate state transitions.

## Phase 4: Explicit Checkpoints

Goal: replace overloaded contract cursor fields with a clearer checkpoint model.

Proposed table:

```sql
chaindexing_checkpoints (
    id BIGSERIAL PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    contract_address VARCHAR,
    handler_kind VARCHAR NOT NULL,
    handler_id VARCHAR NOT NULL,
    next_block_number BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(chain_id, contract_address, handler_kind, handler_id)
)
```

Checkpoint kinds:

- `ingestion`
- `reducer`
- `side_effect`
- `finalized`

Tasks:

- Create checkpoint migration.
- Read from checkpoint table first, with fallback from old cursor fields.
- Write checkpoints transactionally.
- Add migration path from `chaindexing_contract_addresses` cursor fields.
- Keep old fields during transition, then mark them deprecated internally.

Acceptance:

- Existing projects upgrade without losing progress.
- Reset behavior is implemented by checkpoint mutation, not table-wide cursor hacks.
- Reorg repair can move reducer checkpoints backward cleanly.

## Phase 5: Block-Based Reorg Engine

Goal: move from event-diff reorg detection to canonical block tracking.

Proposed table:

```sql
chaindexing_blocks (
    id BIGSERIAL PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash VARCHAR NOT NULL,
    parent_hash VARCHAR NOT NULL,
    status VARCHAR NOT NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(chain_id, block_number, block_hash)
)
```

Tasks:

- Fetch block metadata for every ingested block range.
- Store block hash and parent hash.
- Detect parent/hash mismatch.
- Find the fork point by walking backward.
- Mark replaced blocks as reorged.
- Delete or supersede events from the fork point.
- Delete state versions from the fork point.
- Refresh state views.
- Rewind checkpoints.
- Keep current event-diff detection as an additional safety check during transition.

Acceptance:

- Unit tests simulate a fork with changed block hash.
- State versions after the fork point are removed.
- Handler checkpoints rewind to the fork point.
- Replayed canonical events rebuild the expected latest state.

## Phase 6: Supervised Runtime

Goal: replace loose spawned loops with an observable, cancellable worker runtime.

Tasks:

- Introduce a runtime supervisor for:
  - chain pollers
  - event writers
  - reducer workers
  - side-effect outbox dispatchers
- Add cancellation tokens for shutdown.
- Add bounded concurrency with clear limits.
- Add retry/backoff policy with caps.
- Propagate worker errors instead of silently losing spawned tasks.
- Track worker health in memory first, then optionally in Postgres.

Acceptance:

- A worker panic/error is surfaced in logs and supervisor state.
- Shutdown stops ingestion and handlers cleanly.
- RPC retry loops are bounded or configurable.

## Phase 7: Postgres-First Data Layer

Goal: make the database layer simpler, safer, and more aligned with guarantees.

Tasks:

- Choose one primary Postgres access strategy and make boundaries explicit.
- Use parameterized SQL for dynamic user/state values.
- Use Postgres advisory locks for leader election.
- Use transactions for reducer state changes and checkpoint updates.
- Add migration tests for fresh install and upgrade paths.
- Keep public API free of raw database client details where possible.

Acceptance:

- SQL injection-prone state paths are removed or isolated.
- Leader election does not depend on newest-node semantics.
- Fresh database bootstrap works with one documented command.

## Phase 8: Typed State API

Goal: preserve ergonomics while reducing stringly typed failure modes.

Tasks:

- Add or improve derive macros for state traits.
- Introduce typed filters while preserving current `Filters` as compatibility API.
- Add typed updates while preserving current `Updates`.
- Add read APIs outside handler context:
  - latest by contract
  - latest by chain
  - latest across chains
  - historical at block, if feasible
- Ensure state version ordering uses `block_number`, `transaction_index`, and `log_index`.

Acceptance:

- New examples use typed filters/updates.
- Old examples still work.
- State reads are possible from application code without fabricating handler context.

## Phase 9: Durable Side-Effect Outbox

Goal: make notifications, bridges, and webhooks reliable.

Proposed table:

```sql
chaindexing_outbox (
    id BIGSERIAL PRIMARY KEY,
    idempotency_key VARCHAR NOT NULL UNIQUE,
    chain_id BIGINT NOT NULL,
    contract_address VARCHAR NOT NULL,
    event_id UUID NOT NULL,
    handler_id VARCHAR NOT NULL,
    payload JSONB NOT NULL,
    status VARCHAR NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at TIMESTAMPTZ,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
)
```

Tasks:

- Add an outbox-writing handler API.
- Generate idempotency keys from event identity plus handler id.
- Add dispatcher worker.
- Add retry/backoff/dead-letter behavior.
- Keep direct side-effect handlers for compatibility but document them as legacy/unsafe for strong guarantees.

Acceptance:

- Replaying a block does not duplicate side-effect jobs.
- Dispatcher retries failed jobs.
- Dead-letter status is visible and queryable.

## Phase 10: Public Rust API Cleanup

Goal: make first-run usage obvious.

Target shape:

```rust
Indexer::new(postgres_url)
    .chain(Chain::mainnet(rpc_url))
    .contract(erc721)
    .contract(uniswap_v3)
    .run()
    .await?;
```

Tasks:

- Add `Indexer` builder around existing `Config`.
- Keep `Config` available for advanced usage.
- Validate:
  - at least one chain
  - at least one contract
  - valid ABI strings
  - duplicate contract names
  - duplicate contract addresses per chain
  - missing state migrations for stateful handlers, if detectable
- Improve error types instead of panics/unwraps.

Acceptance:

- A new Rust app can bootstrap with fewer than 30 lines of library setup.
- Misconfiguration errors are actionable.
- Examples move to the new API where it improves clarity.

## Phase 11: Examples And Documentation

Goal: make the project easy to adopt.

Tasks:

- Update `chaindexing-examples/rust/nfts`.
- Update `chaindexing-examples/rust/uniswap`.
- Add examples for:
  - dynamic contract discovery
  - reorg simulation
  - side-effect outbox
  - historical backfill
  - production deployment with Postgres
- Add copy-paste Docker Compose for Postgres.
- Add "first 5 minutes" quickstart.
- Add "production checklist".

Acceptance:

- New user can run an example from scratch with documented commands.
- Examples double as smoke tests in CI.

## Saturday Execution Order

1. Clone and wire `chaindexing-examples` as the acceptance suite.
2. Patch foundational correctness bugs.
3. Add focused unit tests for those fixes.
4. Run `cargo fmt`.
5. Run `cargo test -p chaindexing --lib`.
6. Run or compile the Rust examples against the local crate.
7. Add event identity uniqueness and idempotent insert migrations.
8. Add initial checkpoint table migrations behind compatibility paths.
9. Stop before the block-based reorg rewrite unless earlier phases are green.
10. Record follow-up issues for remaining phases.

## Saturday Definition Of Done

Minimum useful outcome:

- Current crate tests pass.
- Rust examples compile against local crate.
- Side-effect ABI discovery is fixed.
- `ChainState` scoping is fixed.
- Event identity is strengthened in code and tests.
- Event insertion is safe for empty batches.
- Initial event uniqueness/idempotency migration exists.

Stretch outcome:

- Checkpoint table exists.
- Reorg tests are expanded.
- Examples are updated to document the new behavior.

## Known Risks

- Replacing cursor fields too quickly may break existing users; use compatibility reads/writes first.
- Block-based reorg handling is a large change and should not be rushed into the first Saturday slice.
- Raw SQL cleanup may touch a lot of state code; prioritize the highest-risk write/filter paths first.
- Side-effect outbox changes the mental model and should be introduced as a stronger API while keeping compatibility.
- Examples may need dependency/path rewrites before they are useful as smoke tests.

## Immediate Open Questions

- Should the old `Config` API remain the primary API for one more release while `Indexer` is introduced?
- Should Diesel be retained only for static internal tables while dynamic state SQL uses `tokio-postgres`, or should the crate consolidate on one approach?
- What is the desired minimum supported Rust version?
- Should block tracking be enabled by default immediately, or introduced behind a compatibility flag first?
- Should the examples repository become a workspace member during local development, or stay as an external smoke-test repo?
