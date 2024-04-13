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
        let mut futures = self.futures.lock().await;

        join_all(futures.iter_mut()).await;

        *futures = Vec::new();
    }
}
