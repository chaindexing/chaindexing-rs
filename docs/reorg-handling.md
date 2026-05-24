# Reorg Handling

Chaindexing handles EVM reorgs with block canonicalization first, then event replay.
The reorg algorithm is internal; application code chooses only the finality policy.

## How Detection Works

For each chain, Chaindexing stores canonical block headers:

- `chain_id`
- `block_number`
- `block_hash`
- `parent_hash`
- `block_timestamp`
- `status`

When new headers arrive, the ingester compares incoming block hashes with the stored
canonical block at the same height. If a mismatch is found, it walks backward through
parent hashes to find the fork point, marks replaced blocks as `reorged`, and records
the repair in `chaindexing_reorgs`.

## How Event Ingestion Works

Canonical logs are fetched per block hash when the provider supports EIP-234-style
`eth_getLogs` filters:

```text
eth_getLogs({ blockHash, address, topics })
```

This avoids the ambiguity of range-based log reads during reorgs, including the
important case where the correct result is an empty log set. Chaindexing records those
empty scans in `chaindexing_block_scans`, so "we checked this block and found nothing"
is durable state.

If a provider cannot support block-hash log filters, Chaindexing falls back to range
log reads and records scans from the returned data. That fallback is useful, but it has
weaker empty-result guarantees than block-hash reads.

## Repair Flow

When a reorg is detected:

1. A row is written to `chaindexing_reorgs`.
2. Replaced blocks are marked `reorged`.
3. Canonical events from the replaced range are marked `reorged`.
4. Block scans for replaced blocks are marked `reorged`.
5. Ingestion cursors rewind when the main ingestion path detects the fork.
6. Handler state versions are backtracked from the earliest affected block.
7. Handler cursors rewind so canonical events can be replayed.
8. Pending outbox jobs sourced from reorged events are cancelled before dispatch.

Handlers read only canonical events, so state is rebuilt from the surviving canonical
event stream.

## Boundaries

Chaindexing repairs reorgs inside the configured confirmation/finality window. It does
not claim safety beyond retained block, event, and state-version data. For irreversible
business actions, use durable outbox jobs and dispatch them only after a suitable
finality watermark.
