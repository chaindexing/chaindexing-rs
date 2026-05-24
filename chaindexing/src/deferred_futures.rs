use futures_core::Future;
use futures_util::future::join_all;
use std::{pin::Pin, sync::Arc};
use tokio::sync::Mutex;

type DeferredFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a + Send>>;

#[derive(Clone)]
pub struct DeferredFutures<'a> {
    futures: Arc<Mutex<Vec<DeferredFuture<'a>>>>,
}

impl<'a> Default for DeferredFutures<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> DeferredFutures<'a> {
    pub fn new() -> Self {
        Self {
            futures: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub async fn add<'b: 'a, F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'b,
    {
        let mut futures = self.futures.lock().await;
        futures.push(Box::pin(future));
    }
    pub async fn consume(&self) {
        let mut futures = {
            let mut pending_futures = self.futures.lock().await;
            std::mem::take(&mut *pending_futures)
        };

        join_all(futures.iter_mut()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn consume_does_not_hold_queue_lock_while_awaiting_futures() {
        let deferred = DeferredFutures::new();
        let nested_deferred = deferred.clone();
        let completed = Arc::new(AtomicUsize::new(0));
        let completed_for_future = completed.clone();

        deferred
            .add(async move {
                nested_deferred
                    .add(async move {
                        completed_for_future.fetch_add(1, Ordering::SeqCst);
                    })
                    .await;
            })
            .await;

        tokio::time::timeout(Duration::from_millis(100), deferred.consume())
            .await
            .expect("consume should not deadlock while futures add more work");
        deferred.consume().await;

        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }
}
