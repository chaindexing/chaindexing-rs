use chrono::Utc;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A chaindexing node's heartbeat.
/// In a distributed environment, this is useful for managing the indexer's
/// processes manually. A popular motivation to do is to reduce RPC's cost.
#[derive(Clone, Debug)]
pub struct NodeHeartbeat {
    /// Both in milliseconds
    last_keep_alive_at: Arc<Mutex<u64>>,
    active_grace_period: u32,
}

impl NodeHeartbeat {
    /// * `active_grace_period_ms` - how long should the Node wait
    /// till it goes inactive
    pub fn new(active_grace_period_ms: u32) -> Self {
        Self {
            last_keep_alive_at: Arc::new(Mutex::new(Self::now())),
            active_grace_period: active_grace_period_ms,
        }
    }
    /// Keeps your chaindexing node alive
    pub async fn keep_alive(&self) {
        let mut last_keep_alive_at = self.last_keep_alive_at.lock().await;
        *last_keep_alive_at = Self::now();
    }

    fn now() -> u64 {
        Utc::now().timestamp_millis() as u64
    }

    pub async fn is_stale(&self) -> bool {
        !self.is_recent().await
    }
    pub async fn is_recent(&self) -> bool {
        let last_keep_alive_at = *self.last_keep_alive_at.lock().await;
        let min_last_at = Self::now() - (self.active_grace_period as u64);

        last_keep_alive_at > min_last_at
    }
}
