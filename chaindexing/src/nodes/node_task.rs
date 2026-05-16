use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct NodeTask {
    subtasks: Arc<Mutex<Vec<NodeSubtask>>>,
}

struct NodeSubtask {
    name: String,
    handle: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for NodeTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let subtask_count = self.subtasks.try_lock().map(|subtasks| subtasks.len()).ok();

        f.debug_struct("NodeTask").field("subtask_count", &subtask_count).finish()
    }
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
        self.add_named_subtask("unnamed", task).await;
    }

    pub async fn add_named_subtask(
        &self,
        name: impl Into<String>,
        task: tokio::task::JoinHandle<()>,
    ) {
        let mut subtasks = self.subtasks.lock().await;
        subtasks.push(NodeSubtask {
            name: name.into(),
            handle: task,
        });
    }

    pub async fn stop(&self) {
        let mut subtasks = self.subtasks.lock().await;
        for subtask in subtasks.iter() {
            subtask.handle.abort();
        }

        while let Some(subtask) = subtasks.pop() {
            let _ = subtask.handle.await;
        }
    }

    pub async fn collect_errors(&self) -> Vec<String> {
        let mut subtasks = self.subtasks.lock().await;
        let mut errors = vec![];
        let mut index = 0;

        while index < subtasks.len() {
            if !subtasks[index].handle.is_finished() {
                index += 1;
                continue;
            }

            let subtask = subtasks.remove(index);
            match subtask.handle.await {
                Ok(()) => errors.push(format!("Subtask `{}` stopped unexpectedly", subtask.name)),
                Err(join_error) if join_error.is_cancelled() => {}
                Err(join_error) => {
                    errors.push(format!("Subtask `{}` failed: {join_error}", subtask.name))
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collects_failed_subtask_errors() {
        let node_task = NodeTask::new();

        node_task
            .add_named_subtask("panic-task", tokio::spawn(async { panic!("task failed") }))
            .await;

        tokio::task::yield_now().await;
        let errors = node_task.collect_errors().await;

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("panic-task"));
    }

    #[tokio::test]
    async fn stop_aborts_without_reporting_errors() {
        let node_task = NodeTask::new();

        node_task
            .add_named_subtask(
                "running-task",
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }),
            )
            .await;

        node_task.stop().await;

        assert!(node_task.collect_errors().await.is_empty());
    }
}
