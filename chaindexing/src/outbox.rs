use serde::Serialize;

use crate::Event;

/// Result returned after an outbox job is enqueued.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OutboxReceipt {
    pub idempotency_key: String,
}

impl OutboxReceipt {
    pub(crate) fn new(idempotency_key: String) -> Self {
        Self { idempotency_key }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct UnsavedOutboxJob {
    idempotency_key: String,
    chain_id: i64,
    contract_address: String,
    event_id: uuid::Uuid,
    handler_id: String,
    payload: serde_json::Value,
}

impl UnsavedOutboxJob {
    pub fn new<Payload: Serialize>(
        event: &Event,
        handler_id: &str,
        payload: &Payload,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            idempotency_key: idempotency_key(event, handler_id),
            chain_id: event.chain_id,
            contract_address: event.contract_address.clone(),
            event_id: event.id,
            handler_id: handler_id.to_string(),
            payload: serde_json::to_value(payload)?,
        })
    }

    pub fn receipt(&self) -> OutboxReceipt {
        OutboxReceipt::new(self.idempotency_key.clone())
    }

    pub fn insert_query(&self) -> String {
        format!(
            "INSERT INTO chaindexing_outbox
                (idempotency_key, chain_id, contract_address, event_id, handler_id, payload, status)
             VALUES ('{}', {}, '{}', '{}', '{}', '{}'::jsonb, 'pending')
             ON CONFLICT (idempotency_key)
             DO NOTHING",
            escape_sql_literal(&self.idempotency_key),
            self.chain_id,
            escape_sql_literal(&self.contract_address),
            self.event_id,
            escape_sql_literal(&self.handler_id),
            escape_sql_literal(&self.payload.to_string()),
        )
    }
}

fn idempotency_key(event: &Event, handler_id: &str) -> String {
    [
        handler_id,
        &event.chain_id.to_string(),
        &event.contract_address,
        &event.block_hash,
        &event.transaction_hash,
        &event.log_index.to_string(),
    ]
    .join(":")
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_query_escapes_payload_and_handler_id() {
        let job = UnsavedOutboxJob {
            idempotency_key: "handler:1:0xabc:0xblock:0xtx:3".to_string(),
            chain_id: 1,
            contract_address: "0xabc".to_string(),
            event_id: uuid::Uuid::nil(),
            handler_id: "notify'user".to_string(),
            payload: serde_json::json!({ "message": "owner's nft moved" }),
        };

        let query = job.insert_query();

        assert!(query.contains("notify''user"));
        assert!(query.contains("owner''s nft moved"));
        assert!(query.contains("ON CONFLICT (idempotency_key)"));
    }
}
