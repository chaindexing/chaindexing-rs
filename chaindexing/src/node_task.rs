use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Clone)]
pub struct NodeTask {
    tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Default for NodeTask {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTask {
    pub fn new() -> Self {
        NodeTask {
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub async fn add_task(&self, task: tokio::task::JoinHandle<()>) {
        let mut tasks = self.tasks.lock().await;
        tasks.push(task);
    }
    pub async fn stop(&self) {
        let tasks = self.tasks.lock().await;
        for task in tasks.iter() {
            task.abort();
        }
    }
}
