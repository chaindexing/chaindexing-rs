use std::fmt;

const LEGACY_BLOCKS_PER_BATCH: u64 = 450;
const LEGACY_HANDLER_RATE_MS: u64 = 4_000;
const LEGACY_INGESTION_RATE_MS: u64 = 20_000;
const LEGACY_WORKERS: u32 = 4;

/// Behavior-oriented runtime configuration for indexing throughput, polling, and provider limits.
///
/// Prefer starting from a workload profile such as [`RuntimeConfig::realtime`] or
/// [`RuntimeConfig::backfill`], then override explicit resource budgets only when the database or
/// RPC provider capacity is known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    workload: RuntimeWorkload,
    limits: RuntimeLimits,
    rpc: RpcPolicy,
    blocks_per_batch: u64,
    ingestion_poll_interval_ms: u64,
    handler_poll_interval_ms: u64,
}

/// Workload shape selected by a [`RuntimeConfig`] profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeWorkload {
    /// Single-worker, low-fanout behavior for tests and reproducible debugging.
    Deterministic,
    /// Low-latency polling for live app feeds and dashboards.
    Realtime,
    /// Higher batch and fan-out defaults for historical catch-up.
    Backfill,
    /// Moderate catch-up settings that are still suitable once the indexer reaches the head.
    CatchupThenRealtime,
    /// Conservative RPC defaults for expensive or rate-limited providers.
    RpcConstrained,
}

/// Database and worker budgets for the active indexing node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeLimits {
    db_connections: u32,
    max_ingester_workers: u32,
    max_handler_workers: u32,
}

/// JSON-RPC fan-out, rate, and retry policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcPolicy {
    max_in_flight: u32,
    max_per_chain: u32,
    requests_per_second: Option<u32>,
    retry_attempts: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
}

/// Runtime configuration validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfigError {
    field: &'static str,
    message: &'static str,
}

impl RuntimeConfigError {
    pub(crate) fn new(field: &'static str, message: &'static str) -> Self {
        Self { field, message }
    }
}

impl fmt::Display for RuntimeConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.field, self.message)
    }
}

impl std::error::Error for RuntimeConfigError {}

impl RuntimeConfig {
    pub(crate) fn legacy_compatible() -> Self {
        Self {
            workload: RuntimeWorkload::Realtime,
            limits: RuntimeLimits {
                db_connections: LEGACY_WORKERS,
                max_ingester_workers: LEGACY_WORKERS,
                max_handler_workers: LEGACY_WORKERS,
            },
            rpc: RpcPolicy::balanced(),
            blocks_per_batch: LEGACY_BLOCKS_PER_BATCH,
            ingestion_poll_interval_ms: LEGACY_INGESTION_RATE_MS,
            handler_poll_interval_ms: LEGACY_HANDLER_RATE_MS,
        }
    }

    /// Single-worker profile for tests and reproducible debugging.
    pub fn deterministic() -> Self {
        Self {
            workload: RuntimeWorkload::Deterministic,
            limits: RuntimeLimits::conservative()
                .db_connections(2)
                .max_ingester_workers(1)
                .max_handler_workers(1),
            rpc: RpcPolicy::limited().max_in_flight(1).max_per_chain(1),
            blocks_per_batch: 25,
            ingestion_poll_interval_ms: 1_000,
            handler_poll_interval_ms: 1_000,
        }
    }

    /// Low-latency profile for live app feeds and dashboards.
    pub fn realtime() -> Self {
        Self {
            workload: RuntimeWorkload::Realtime,
            limits: RuntimeLimits::balanced(),
            rpc: RpcPolicy::balanced(),
            blocks_per_batch: 450,
            ingestion_poll_interval_ms: 2_000,
            handler_poll_interval_ms: 1_000,
        }
    }

    /// Throughput-oriented profile for historical catch-up.
    pub fn backfill() -> Self {
        Self {
            workload: RuntimeWorkload::Backfill,
            limits: RuntimeLimits::throughput(),
            rpc: RpcPolicy::throughput(),
            blocks_per_batch: 2_000,
            ingestion_poll_interval_ms: 250,
            handler_poll_interval_ms: 250,
        }
    }

    /// Profile for apps that need to catch up quickly, then remain suitable for realtime indexing.
    pub fn catchup_then_realtime() -> Self {
        Self {
            workload: RuntimeWorkload::CatchupThenRealtime,
            limits: RuntimeLimits::balanced().max_ingester_workers(6).max_handler_workers(6),
            rpc: RpcPolicy::balanced().max_in_flight(32).max_per_chain(8),
            blocks_per_batch: 1_000,
            ingestion_poll_interval_ms: 1_000,
            handler_poll_interval_ms: 500,
        }
    }

    /// Conservative profile for expensive or rate-limited RPC providers.
    pub fn rpc_constrained() -> Self {
        Self {
            workload: RuntimeWorkload::RpcConstrained,
            limits: RuntimeLimits::conservative(),
            rpc: RpcPolicy::limited().max_in_flight(4).max_per_chain(2).requests_per_second(5),
            blocks_per_batch: 200,
            ingestion_poll_interval_ms: 10_000,
            handler_poll_interval_ms: 4_000,
        }
    }

    pub fn limits(mut self, limits: RuntimeLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn rpc(mut self, rpc: RpcPolicy) -> Self {
        self.rpc = rpc;
        self
    }

    pub fn blocks_per_batch(mut self, blocks_per_batch: u64) -> Self {
        self.blocks_per_batch = blocks_per_batch;
        self
    }

    pub fn ingestion_poll_interval_ms(mut self, ingestion_poll_interval_ms: u64) -> Self {
        self.ingestion_poll_interval_ms = ingestion_poll_interval_ms;
        self
    }

    pub fn handler_poll_interval_ms(mut self, handler_poll_interval_ms: u64) -> Self {
        self.handler_poll_interval_ms = handler_poll_interval_ms;
        self
    }

    pub fn workload(&self) -> RuntimeWorkload {
        self.workload
    }

    pub fn limits_ref(&self) -> &RuntimeLimits {
        &self.limits
    }

    pub fn rpc_ref(&self) -> &RpcPolicy {
        &self.rpc
    }

    pub fn blocks_per_batch_value(&self) -> u64 {
        self.blocks_per_batch
    }

    pub fn ingestion_poll_interval_ms_value(&self) -> u64 {
        self.ingestion_poll_interval_ms
    }

    pub fn handler_poll_interval_ms_value(&self) -> u64 {
        self.handler_poll_interval_ms
    }

    pub fn validate(&self) -> Result<(), RuntimeConfigError> {
        non_zero(self.blocks_per_batch, "blocks_per_batch")?;
        non_zero(
            self.ingestion_poll_interval_ms,
            "ingestion_poll_interval_ms",
        )?;
        non_zero(self.handler_poll_interval_ms, "handler_poll_interval_ms")?;
        self.limits.validate()?;
        self.rpc.validate()?;

        Ok(())
    }

    pub fn resolved_summary(&self) -> String {
        format!(
            "workload={:?}, db_connections={}, max_ingester_workers={}, max_handler_workers={}, max_rpc_in_flight={}, max_rpc_per_chain={}, rpc_requests_per_second={:?}, blocks_per_batch={}, ingestion_poll_interval_ms={}, handler_poll_interval_ms={}",
            self.workload,
            self.limits.db_connections,
            self.limits.max_ingester_workers,
            self.limits.max_handler_workers,
            self.rpc.max_in_flight,
            self.rpc.max_per_chain,
            self.rpc.requests_per_second,
            self.blocks_per_batch,
            self.ingestion_poll_interval_ms,
            self.handler_poll_interval_ms,
        )
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::legacy_compatible()
    }
}

impl RuntimeLimits {
    /// Conservative local/default resource budget.
    pub fn conservative() -> Self {
        Self {
            db_connections: 4,
            max_ingester_workers: 2,
            max_handler_workers: 2,
        }
    }

    /// Balanced resource budget for common realtime indexing workloads.
    pub fn balanced() -> Self {
        Self {
            db_connections: 8,
            max_ingester_workers: 4,
            max_handler_workers: 4,
        }
    }

    /// Larger resource budget for catch-up and backfill workloads.
    pub fn throughput() -> Self {
        Self {
            db_connections: 16,
            max_ingester_workers: 8,
            max_handler_workers: 8,
        }
    }

    /// Sets the pooled Postgres connection budget for ingestion/supervisor work.
    pub fn db_connections(mut self, db_connections: u32) -> Self {
        self.db_connections = db_connections;
        self
    }

    /// Sets the maximum number of ingestion workers before RPC-budget caps are applied.
    pub fn max_ingester_workers(mut self, max_ingester_workers: u32) -> Self {
        self.max_ingester_workers = max_ingester_workers;
        self
    }

    /// Sets the maximum number of handler workers.
    pub fn max_handler_workers(mut self, max_handler_workers: u32) -> Self {
        self.max_handler_workers = max_handler_workers;
        self
    }

    pub fn db_connections_value(&self) -> u32 {
        self.db_connections
    }

    pub fn max_ingester_workers_value(&self) -> u32 {
        self.max_ingester_workers
    }

    pub fn max_handler_workers_value(&self) -> u32 {
        self.max_handler_workers
    }

    fn validate(&self) -> Result<(), RuntimeConfigError> {
        non_zero(self.db_connections, "db_connections")?;
        non_zero(self.max_ingester_workers, "max_ingester_workers")?;
        non_zero(self.max_handler_workers, "max_handler_workers")
    }
}

impl RpcPolicy {
    /// Conservative RPC fan-out and retry defaults.
    pub fn limited() -> Self {
        Self {
            max_in_flight: 8,
            max_per_chain: 4,
            requests_per_second: None,
            retry_attempts: 5,
            base_backoff_ms: 1_000,
            max_backoff_ms: 30_000,
        }
    }

    /// Balanced RPC fan-out and retry defaults.
    pub fn balanced() -> Self {
        Self {
            max_in_flight: 16,
            max_per_chain: 4,
            requests_per_second: None,
            retry_attempts: 5,
            base_backoff_ms: 1_000,
            max_backoff_ms: 30_000,
        }
    }

    /// Higher RPC fan-out defaults for providers and workloads that can absorb it.
    pub fn throughput() -> Self {
        Self {
            max_in_flight: 64,
            max_per_chain: 16,
            requests_per_second: None,
            retry_attempts: 5,
            base_backoff_ms: 500,
            max_backoff_ms: 30_000,
        }
    }

    /// Sets the global JSON-RPC in-flight budget used to cap effective ingestion workers.
    pub fn max_in_flight(mut self, max_in_flight: u32) -> Self {
        self.max_in_flight = max_in_flight;
        self
    }

    /// Sets the maximum concurrent JSON-RPC log requests issued by a single chain worker.
    pub fn max_per_chain(mut self, max_per_chain: u32) -> Self {
        self.max_per_chain = max_per_chain;
        self
    }

    /// Sets an optional JSON-RPC request-rate hint for rate-limited providers.
    pub fn requests_per_second(mut self, requests_per_second: u32) -> Self {
        self.requests_per_second = Some(requests_per_second);
        self
    }

    /// Sets the total number of provider attempts before returning the last error.
    pub fn retry_attempts(mut self, retry_attempts: u32) -> Self {
        self.retry_attempts = retry_attempts;
        self
    }

    /// Sets the initial retry backoff delay in milliseconds.
    pub fn base_backoff_ms(mut self, base_backoff_ms: u64) -> Self {
        self.base_backoff_ms = base_backoff_ms;
        self
    }

    /// Sets the maximum retry backoff delay in milliseconds.
    pub fn max_backoff_ms(mut self, max_backoff_ms: u64) -> Self {
        self.max_backoff_ms = max_backoff_ms;
        self
    }

    pub fn max_in_flight_value(&self) -> u32 {
        self.max_in_flight
    }

    pub fn max_per_chain_value(&self) -> u32 {
        self.max_per_chain
    }

    pub fn requests_per_second_value(&self) -> Option<u32> {
        self.requests_per_second
    }

    pub fn retry_attempts_value(&self) -> u32 {
        self.retry_attempts
    }

    pub fn base_backoff_ms_value(&self) -> u64 {
        self.base_backoff_ms
    }

    pub fn max_backoff_ms_value(&self) -> u64 {
        self.max_backoff_ms
    }

    fn validate(&self) -> Result<(), RuntimeConfigError> {
        non_zero(self.max_in_flight, "max_rpc_in_flight")?;
        non_zero(self.max_per_chain, "max_rpc_per_chain")?;
        non_zero(self.retry_attempts, "rpc_retry_attempts")?;
        non_zero(self.base_backoff_ms, "rpc_base_backoff_ms")?;
        non_zero(self.max_backoff_ms, "rpc_max_backoff_ms")?;

        if self.max_per_chain > self.max_in_flight {
            return Err(RuntimeConfigError::new(
                "max_rpc_per_chain",
                "must be less than or equal to max_rpc_in_flight",
            ));
        }

        if let Some(requests_per_second) = self.requests_per_second {
            non_zero(requests_per_second, "rpc_requests_per_second")?;
        }

        if self.base_backoff_ms > self.max_backoff_ms {
            return Err(RuntimeConfigError::new(
                "rpc_base_backoff_ms",
                "must be less than or equal to rpc_max_backoff_ms",
            ));
        }

        Ok(())
    }
}

fn non_zero<T>(value: T, field: &'static str) -> Result<(), RuntimeConfigError>
where
    T: PartialEq + From<u8>,
{
    if value == T::from(0) {
        Err(RuntimeConfigError::new(field, "must be greater than zero"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_profile_uses_single_worker_limits() {
        let config = RuntimeConfig::deterministic();

        assert_eq!(config.workload(), RuntimeWorkload::Deterministic);
        assert_eq!(config.limits_ref().max_ingester_workers_value(), 1);
        assert_eq!(config.limits_ref().max_handler_workers_value(), 1);
        assert_eq!(config.rpc_ref().max_in_flight_value(), 1);
        assert_eq!(config.rpc_ref().max_per_chain_value(), 1);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn advanced_overrides_win_over_profile_defaults() {
        let config = RuntimeConfig::backfill()
            .limits(
                RuntimeLimits::throughput()
                    .db_connections(24)
                    .max_ingester_workers(12)
                    .max_handler_workers(10),
            )
            .rpc(RpcPolicy::throughput().max_in_flight(96).max_per_chain(24))
            .blocks_per_batch(4_000);

        assert_eq!(config.limits_ref().db_connections_value(), 24);
        assert_eq!(config.limits_ref().max_ingester_workers_value(), 12);
        assert_eq!(config.limits_ref().max_handler_workers_value(), 10);
        assert_eq!(config.rpc_ref().max_in_flight_value(), 96);
        assert_eq!(config.rpc_ref().max_per_chain_value(), 24);
        assert_eq!(config.blocks_per_batch_value(), 4_000);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_rpc_per_chain_above_global_in_flight_limit() {
        let error = RuntimeConfig::realtime()
            .rpc(RpcPolicy::limited().max_in_flight(4).max_per_chain(8))
            .validate()
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "max_rpc_per_chain must be less than or equal to max_rpc_in_flight"
        );
    }

    #[test]
    fn resolved_summary_names_behavior_and_limits() {
        let summary = RuntimeConfig::rpc_constrained().resolved_summary();

        assert!(summary.contains("workload=RpcConstrained"));
        assert!(summary.contains("max_rpc_per_chain=2"));
        assert!(summary.contains("rpc_requests_per_second=Some(5)"));
    }
}
