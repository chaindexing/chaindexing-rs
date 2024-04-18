use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct NodeTask {
    subtasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Default for NodeTask {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTask {
    pub fn new() -> Self {
        NodeTask {
            subtasks: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub async fn add_subtask(&self, task: tokio::task::JoinHandle<()>) {
        let mut subtasks = self.subtasks.lock().await;
        subtasks.push(task);
    }
    pub async fn stop(&self) {
        let subtasks = self.subtasks.lock().await;
        for subtask in subtasks.iter() {
            subtask.abort();
        }
    }
}
