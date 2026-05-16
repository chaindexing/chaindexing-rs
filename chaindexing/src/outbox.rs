use serde::Deserialize;
use serde::Serialize;

use crate::{Event, ExecutesWithRawQuery, HasRawQueryClient, LoadsDataWithRawQuery, PostgresRepo};

/// A durable outbox job ready to dispatch.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OutboxJob {
    pub id: i64,
    pub idempotency_key: String,
    pub chain_id: i64,
    pub contract_address: String,
    pub event_id: uuid::Uuid,
    pub handler_id: String,
    pub payload: serde_json::Value,
    pub attempt_count: i32,
}

/// Configuration for dispatching pending outbox jobs.
#[derive(Debug, Clone, Copy)]
pub struct OutboxDispatchConfig {
    pub batch_size: u64,
    pub max_attempts: u32,
    pub base_retry_delay_secs: u64,
}

impl Default for OutboxDispatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            max_attempts: 10,
            base_retry_delay_secs: 5,
        }
    }
}

/// Dispatches durable outbox jobs to external systems.
#[crate::augmenting_std::async_trait]
pub trait OutboxDispatcher: Send + Sync {
    async fn dispatch(&self, job: OutboxJob) -> Result<(), String>;
}

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

/// Leases pending outbox jobs, dispatches them, and updates their status.
pub async fn dispatch_pending_outbox_jobs<Dispatcher: OutboxDispatcher + ?Sized>(
    postgres_url: &str,
    dispatcher: &Dispatcher,
    config: OutboxDispatchConfig,
) -> usize {
    let repo = PostgresRepo::new(postgres_url);
    let client = repo.get_client().await;
    let jobs: Vec<OutboxJob> =
        PostgresRepo::load_data_list(&client, &lease_pending_jobs_query(config.batch_size)).await;
    let dispatched_count = jobs.len();

    for job in jobs {
        match dispatcher.dispatch(job.clone()).await {
            Ok(()) => PostgresRepo::execute(&client, &mark_delivered_query(job.id)).await,
            Err(error) => {
                PostgresRepo::execute(&client, &mark_failed_query(&job, &error, config)).await
            }
        }
    }

    dispatched_count
}

fn lease_pending_jobs_query(limit: u64) -> String {
    format!(
        "UPDATE chaindexing_outbox
         SET status = 'dispatching', updated_at = NOW()
         WHERE id IN (
             SELECT id
             FROM chaindexing_outbox
             WHERE status = 'pending'
               AND (next_attempt_at IS NULL OR next_attempt_at <= NOW())
             ORDER BY id ASC
             LIMIT {limit}
             FOR UPDATE SKIP LOCKED
         )
         RETURNING id, idempotency_key, chain_id, contract_address, event_id, handler_id, payload, attempt_count"
    )
}

fn mark_delivered_query(id: i64) -> String {
    format!(
        "UPDATE chaindexing_outbox
         SET status = 'delivered',
             last_error = NULL,
             next_attempt_at = NULL,
             updated_at = NOW()
         WHERE id = {id}"
    )
}

fn mark_failed_query(job: &OutboxJob, error: &str, config: OutboxDispatchConfig) -> String {
    let next_attempt_count = job.attempt_count + 1;
    let max_attempts = config.max_attempts.max(1) as i32;
    let status = if next_attempt_count >= max_attempts {
        "dead"
    } else {
        "pending"
    };
    let retry_delay_secs = config.base_retry_delay_secs
        * 2u64.pow((next_attempt_count as u32).saturating_sub(1).min(6));
    let next_attempt_at = if status == "dead" {
        "NULL".to_string()
    } else {
        format!("NOW() + INTERVAL '{retry_delay_secs} seconds'")
    };

    format!(
        "UPDATE chaindexing_outbox
         SET status = '{status}',
             attempt_count = {next_attempt_count},
             last_error = '{}',
             next_attempt_at = {next_attempt_at},
             updated_at = NOW()
         WHERE id = {}",
        escape_sql_literal(error),
        job.id
    )
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

    #[test]
    fn lease_pending_jobs_query_marks_jobs_as_dispatching() {
        let query = lease_pending_jobs_query(10);

        assert!(query.contains("SET status = 'dispatching'"));
        assert!(query.contains("FOR UPDATE SKIP LOCKED"));
        assert!(query.contains("LIMIT 10"));
    }

    #[test]
    fn mark_failed_query_dead_letters_after_max_attempts() {
        let job = OutboxJob {
            id: 7,
            idempotency_key: "key".to_string(),
            chain_id: 1,
            contract_address: "0xabc".to_string(),
            event_id: uuid::Uuid::nil(),
            handler_id: "handler".to_string(),
            payload: serde_json::json!({}),
            attempt_count: 2,
        };

        let query = mark_failed_query(
            &job,
            "provider's API failed",
            OutboxDispatchConfig {
                max_attempts: 3,
                ..Default::default()
            },
        );

        assert!(query.contains("status = 'dead'"));
        assert!(query.contains("attempt_count = 3"));
        assert!(query.contains("provider''s API failed"));
        assert!(query.contains("next_attempt_at = NULL"));
    }
}
