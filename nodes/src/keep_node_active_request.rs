#[derive(Clone)]
pub struct KeepNodeActiveRequest {
    /// Both in milliseconds
    last_refreshed_at_and_active_grace_period: Arc<Mutex<(u64, u32)>>,
}

impl KeepNodeActiveRequest {
    /// * `active_grace_period_ms` - how long should the Node wait
    /// till it goes inactive
    pub fn new(active_grace_period_ms: u32) -> Self {
        Self {
            last_refreshed_at_and_active_grace_period: Arc::new(Mutex::new((
                Self::now(),
                active_grace_period_ms,
            ))),
        }
    }
    pub async fn refresh(&self) {
        let mut last_refreshed_at_and_active_grace_period =
            self.last_refreshed_at_and_active_grace_period.lock().await;
        *last_refreshed_at_and_active_grace_period =
            (Self::now(), last_refreshed_at_and_active_grace_period.1);
    }

    fn now() -> u64 {
        Utc::now().timestamp_millis() as u64
    }

    async fn is_stale(&self) -> bool {
        !self.is_recent().await
    }
    async fn is_recent(&self) -> bool {
        let (last_refreshed_at, active_grace_period) =
            *self.last_refreshed_at_and_active_grace_period.lock().await;
        let min_last_refreshed_at = Self::now() - (active_grace_period as u64);

        last_refreshed_at > min_last_refreshed_at
    }
}
