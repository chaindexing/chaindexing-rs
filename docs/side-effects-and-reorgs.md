# Side Effects and Reorgs

Direct side-effect handlers are supported for compatibility, but durable side effects
should go through the Postgres outbox. The outbox gives each job an idempotency key
derived from the canonical event identity and records the source block metadata.

## Enqueue Jobs

```rust
use chaindexing::{SideEffectContext, SideEffectFinality};

context
    .enqueue_outbox_with_configured_finality("transfer-webhook", &payload)
    .await;
```

You can also override finality per job:

```rust
use chaindexing::SideEffectFinality;

context
    .enqueue_outbox_with_finality(
        "transfer-webhook",
        &payload,
        SideEffectFinality::Safe,
    )
    .await;
```

For low-risk or fully idempotent effects, `enqueue_outbox` uses immediate eligibility:

```rust
context.enqueue_outbox("cache-refresh", &payload).await;
```

## Dispatch Jobs

Dispatchers should pass the latest finality watermarks they trust:

```rust
use chaindexing::{
    dispatch_pending_outbox_jobs, OutboxDispatchConfig, OutboxFinalityWatermark,
};

let config = OutboxDispatchConfig::default().with_finality_watermark(
    OutboxFinalityWatermark {
        chain_id: 1,
        latest_block_number: Some(latest),
        safe_block_number: Some(safe),
        finalized_block_number: Some(finalized),
    },
);

dispatch_pending_outbox_jobs(&database_url, &dispatcher, config).await;
```

Jobs whose `required_finality` is not satisfied remain pending. Pending or dispatching
jobs sourced from events later marked `reorged` are moved to `cancelled_reorg` before
new leases are issued.

## Recommendations

| Effect type | Recommendation |
| --- | --- |
| UI cache refresh | Immediate or confirmations |
| Email/notification | Safe |
| Webhook to customer system | Safe or finalized |
| Payment/claim/settlement | Finalized |
| Bridge or irreversible off-chain write | Finalized |

External systems should still consume outbox jobs idempotently using
`job.idempotency_key`. Outbox dispatch is at-least-once by design.
