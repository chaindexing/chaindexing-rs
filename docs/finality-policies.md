# Finality Policies

Chaindexing exposes presets for product-level behavior and keeps the low-level reorg
algorithm automatic.

## Presets

```rust
use chaindexing::ReorgMode;

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Realtime);

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Balanced);

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::FinalityFirst);
```

| Preset | Use when | Indexing behavior | Side-effect default |
| --- | --- | --- | --- |
| `Realtime` | Dashboards, feeds, live UI | Index near latest and repair by replay | Safe |
| `Balanced` | Analytics, reporting, operational apps | Use `safe` when provider supports it | Safe |
| `FinalityFirst` | Payments, claims, settlement | Use `finalized` when provider supports it | Finalized |

The default remains low-latency for compatibility. Production systems that value
stability over freshness should explicitly choose `Balanced` or `FinalityFirst`.

## Explicit Overrides

Presets are intentionally small. If a product needs a specific policy, override it:

```rust
use chaindexing::{IndexingFinality, ReorgMode, SideEffectFinality};

Indexer::new(&database_url)
    .reorg_mode(ReorgMode::Balanced)
    .indexing_finality(IndexingFinality::LatestWithConfirmations(12))
    .side_effect_finality(SideEffectFinality::Finalized);
```

## Use Case Guide

| Use case | Recommended policy |
| --- | --- |
| Activity feed | `Realtime` |
| Portfolio or dashboard | `Realtime` or `Balanced` |
| Analytics/reporting | `Balanced` |
| Accounting/balances | `Balanced` or `FinalityFirst` |
| Webhooks/emails | Index with `Realtime` or `Balanced`, dispatch with `Safe` or `Finalized` |
| Payments/claims | `FinalityFirst` |
| Historical backfill | `FinalityFirst` or explicit finalized ingestion |
| L2 indexing | Chain-specific preset or explicit confirmations |

## Provider Fallbacks

Ethereum RPC providers may differ in support for `safe`, `finalized`, and block-hash
log filters. Chaindexing uses the strongest available path and falls back where needed.
When `safe` or `finalized` is unavailable, the system can still run with confirmation
depths, but the finality guarantee is only as strong as that fallback.
