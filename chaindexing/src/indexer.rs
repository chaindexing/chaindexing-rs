use std::fmt::Debug;

use crate::{
    index_states, start_indexing, Chain, ChaindexingError, Config, Contract, IndexedDataConfig,
    IndexingFinality, IndexingHandle, PostgresRepo, PostgresTlsConfig, ReorgMode, RuntimeConfig,
    SideEffectFinality,
};

/// High-level builder for configuring and running a Postgres-backed indexer.
#[derive(Clone, Debug)]
pub struct Indexer<SharedState: Sync + Send + Clone = ()> {
    config: Config<SharedState>,
}

impl Indexer<()> {
    /// Creates an indexer backed by Postgres.
    pub fn new(postgres_url: &str) -> Self {
        Self::postgres(postgres_url)
    }

    /// Creates an indexer backed by Postgres with explicit TLS configuration.
    pub fn new_with_tls(postgres_url: &str, tls_config: PostgresTlsConfig) -> Self {
        Self::postgres_with_tls(postgres_url, tls_config)
    }
}

impl<SharedState: Sync + Send + Clone> Indexer<SharedState> {
    /// Creates a typed indexer backed by Postgres.
    pub fn postgres(postgres_url: &str) -> Self {
        Self {
            config: Config::new(PostgresRepo::new(postgres_url)),
        }
    }

    /// Creates a typed indexer backed by Postgres with explicit TLS configuration.
    pub fn postgres_with_tls(postgres_url: &str, tls_config: PostgresTlsConfig) -> Self {
        Self {
            config: Config::new(PostgresRepo::new_with_tls(postgres_url, tls_config)),
        }
    }

    /// Creates a typed indexer backed by Postgres with shared state for side-effect handlers.
    pub fn new_with_shared_state(postgres_url: &str, initial_state: SharedState) -> Self {
        Self::postgres(postgres_url).initial_state(initial_state)
    }

    /// Creates a typed indexer backed by Postgres with shared state and explicit TLS configuration.
    pub fn new_with_shared_state_and_tls(
        postgres_url: &str,
        initial_state: SharedState,
        tls_config: PostgresTlsConfig,
    ) -> Self {
        Self::postgres_with_tls(postgres_url, tls_config).initial_state(initial_state)
    }

    /// Wraps an existing advanced `Config`.
    pub fn from_config(config: Config<SharedState>) -> Self {
        Self { config }
    }

    /// Adds an EVM chain to index.
    pub fn chain(self, chain: Chain) -> Self {
        Self {
            config: self.config.add_chain(chain),
        }
    }

    /// Adds a contract specification to index.
    pub fn contract(self, contract: Contract<SharedState>) -> Self {
        Self {
            config: self.config.add_contract(contract),
        }
    }

    /// Sets the number of blocks fetched and handled per batch.
    pub fn blocks_per_batch(self, blocks_per_batch: u64) -> Self {
        Self {
            config: self.config.with_blocks_per_batch(blocks_per_batch),
        }
    }

    /// Sets how many confirmations are required before reorg checks run.
    pub fn min_confirmations(self, min_confirmation_count: u8) -> Self {
        Self {
            config: self.config.with_min_confirmation_count(min_confirmation_count),
        }
    }

    /// Uses a preset reorg/finality posture.
    pub fn reorg_mode(self, reorg_mode: ReorgMode) -> Self {
        Self {
            config: self.config.with_reorg_mode(reorg_mode),
        }
    }

    /// Overrides the preset's event ingestion finality policy.
    pub fn indexing_finality(self, indexing_finality: IndexingFinality) -> Self {
        Self {
            config: self.config.with_indexing_finality(indexing_finality),
        }
    }

    /// Overrides the preset's durable side-effect dispatch finality policy.
    pub fn side_effect_finality(self, side_effect_finality: SideEffectFinality) -> Self {
        Self {
            config: self.config.with_side_effect_finality(side_effect_finality),
        }
    }

    /// Indexes full JSON-RPC transaction payloads alongside events.
    pub fn raw_transactions(self) -> Self {
        Self {
            config: self.config.with_raw_transaction_indexing(),
        }
    }

    /// Indexes per-block call traces alongside events.
    pub fn call_traces(self) -> Self {
        Self {
            config: self.config.with_call_trace_indexing(),
        }
    }

    /// Replaces the optional indexed-data configuration.
    pub fn indexed_data(self, indexed_data_config: IndexedDataConfig) -> Self {
        Self {
            config: self.config.with_indexed_data(indexed_data_config),
        }
    }

    /// Sets the ingestion loop interval in milliseconds.
    pub fn ingestion_rate_ms(self, ingestion_rate_ms: u64) -> Self {
        Self {
            config: self.config.with_ingestion_rate_ms(ingestion_rate_ms),
        }
    }

    /// Sets the handler loop interval in milliseconds.
    pub fn handler_rate_ms(self, handler_rate_ms: u64) -> Self {
        Self {
            config: self.config.with_handler_rate_ms(handler_rate_ms),
        }
    }

    /// Configures how many chain batches can be processed concurrently.
    pub fn chain_concurrency(self, chain_concurrency: u32) -> Self {
        Self {
            config: self.config.with_chain_concurrency(chain_concurrency),
        }
    }

    /// Configures how many ingestion workers can run concurrently.
    pub fn ingester_concurrency(self, ingester_concurrency: u32) -> Self {
        Self {
            config: self.config.with_ingester_concurrency(ingester_concurrency),
        }
    }

    /// Configures how many handler workers can run concurrently.
    pub fn handler_concurrency(self, handler_concurrency: u32) -> Self {
        Self {
            config: self.config.with_handler_concurrency(handler_concurrency),
        }
    }

    /// Configures Chaindexing with a behavior-oriented runtime profile plus
    /// explicit resource and RPC limits.
    pub fn runtime(self, runtime_config: RuntimeConfig) -> Self {
        Self {
            config: self.config.with_runtime_config(runtime_config),
        }
    }

    /// Sets the Postgres advisory lock id used for leader election.
    pub fn leader_lock_id(self, leader_lock_id: i64) -> Self {
        Self {
            config: self.config.with_leader_lock_id(leader_lock_id),
        }
    }

    /// Provides shared state for side-effect handlers.
    pub fn initial_state(self, initial_state: SharedState) -> Self {
        Self {
            config: self.config.with_initial_state(initial_state),
        }
    }

    /// Restarts pure handler indexing from scratch when the count changes.
    pub fn reset(self, count: u64) -> Self {
        Self {
            config: self.config.reset(count),
        }
    }

    /// Restarts pure and side-effect handlers from scratch when the count changes.
    pub fn reset_including_side_effects_dangerously(self, count: u64) -> Self {
        Self {
            config: self.config.reset_including_side_effects_dangerously(count),
        }
    }

    /// Returns the underlying advanced config.
    pub fn into_config(self) -> Config<SharedState> {
        self.config
    }

    /// Borrows the underlying advanced config.
    pub fn config(&self) -> &Config<SharedState> {
        &self.config
    }
}

impl<SharedState: Sync + Send + Clone + Debug + 'static> Indexer<SharedState> {
    /// Starts the configured indexer workers and blocks until Ctrl-C triggers a graceful shutdown.
    ///
    /// Use [`Indexer::start`] instead when embedding Chaindexing in a service that owns its own
    /// shutdown signal or lifecycle supervisor.
    pub async fn run(self) -> Result<(), ChaindexingError> {
        index_states(&self.config).await
    }

    /// Starts the configured indexer workers and returns a lifecycle handle immediately.
    pub async fn start(self) -> Result<IndexingHandle, ChaindexingError> {
        start_indexing(&self.config).await
    }
}
