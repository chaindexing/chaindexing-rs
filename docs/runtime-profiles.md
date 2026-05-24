# Runtime Profiles

Chaindexing runtime profiles describe workload behavior. They deliberately avoid names like
`development`, `staging`, or `production` because those environments do not imply stable resource
budgets.

## Profiles

| Profile | Use case | Bias |
| --- | --- | --- |
| `RuntimeConfig::deterministic()` | Tests and reproducible debugging | Single ingestion worker, single handler worker, one RPC request in flight. |
| `RuntimeConfig::realtime()` | Live dApp feeds and dashboards | Low polling latency with moderate batch and worker defaults. |
| `RuntimeConfig::backfill()` | Historical catch-up | Larger batches, more workers, and higher RPC fan-out. |
| `RuntimeConfig::catchup_then_realtime()` | Apps that must catch up then stay fresh | Moderate-high catch-up limits without the largest backfill defaults. |
| `RuntimeConfig::rpc_constrained()` | Expensive or rate-limited providers | Low RPC fan-out, slower polling, optional request-rate budget. |

## Policy Axes

Runtime config has separate axes so developers can compose intent without hidden environment
assumptions:

- Workload: `realtime`, `backfill`, `catchup_then_realtime`, `deterministic`, or `rpc_constrained`.
- Limits: pooled database connections, max ingestion workers, max handler workers.
- RPC policy: max in-flight requests, per-chain cap, optional requests-per-second hint, and retry/backoff.

`db_connections` controls the Postgres pool used by pooled ingestion/supervisor work. Handler raw
clients are bounded by `max_handler_workers`, so it is a capacity hint rather than a process-wide
connection ceiling.

Existing leader election and `max_concurrent_node_count` still control multi-process ownership.
Runtime profiles tune the active node's work budget instead of trying to infer capacity from
environment names.

## Ordering Guarantees

Handler concurrency is partition-aware:

- `ContractState` must remain ordered by `(chain_id, contract_address)`.
- `ChainState` must remain ordered by `chain_id`.
- `MultiChainState` remains serialized/global unless the implementation can prove order-agnostic execution.
- Durable side effects should use the outbox. Direct side-effect handlers are compatibility-oriented and cannot provide exactly-once network calls.

Increasing `max_handler_workers` helps only when there are independent handler partitions. A single
hot ordered partition still processes in order.

The runtime derives an effective ingestion worker cap from the RPC budget:

```text
effective_ingester_workers <= max_rpc_in_flight / max_rpc_per_chain
```

This keeps profile overrides predictable: increasing workers without increasing RPC budget will not
silently overload the provider.

## Examples

```rust
use chaindexing::{RpcPolicy, RuntimeConfig, RuntimeLimits};

let realtime = RuntimeConfig::realtime();

let backfill = RuntimeConfig::backfill()
    .limits(RuntimeLimits::throughput().db_connections(16))
    .rpc(RpcPolicy::throughput().max_in_flight(64).max_per_chain(16));

let rpc_constrained = RuntimeConfig::rpc_constrained()
    .rpc(RpcPolicy::limited().max_in_flight(4).max_per_chain(2).requests_per_second(5));
```

At startup Chaindexing logs the resolved runtime config so deployed behavior can be inspected
without reverse-engineering chained builders.
