use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
pub struct NodeTask {
    subtasks: Arc<Mutex<Vec<NodeSubtask>>>,
    cancellation_token: CancellationToken,
}

struct NodeSubtask {
    name: String,
    handle: tokio::task::JoinHandle<NodeSubtaskResult>,
}

pub(crate) type NodeSubtaskResult = Result<(), NodeSubtaskFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NodeSubtaskFailure {
    pub(crate) message: String,
}

impl std::fmt::Display for NodeTaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeTaskError::StoppedUnexpectedly { task_name } => {
                write!(f, "Subtask `{task_name}` stopped unexpectedly")
            }
            NodeTaskError::Failed { task_name, message } => {
                write!(f, "Subtask `{task_name}` failed: {message}")
            }
            NodeTaskError::Panicked { task_name, message } => {
                write!(f, "Subtask `{task_name}` panicked: {message}")
            }
        }
    }
}

impl std::error::Error for NodeTaskError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NodeTaskError {
    StoppedUnexpectedly { task_name: String },
    Failed { task_name: String, message: String },
    Panicked { task_name: String, message: String },
}

#[derive(Clone, Debug)]
pub struct CancellationToken {
    is_cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            is_cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        self.notify.notified().await;
    }
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
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    pub(crate) async fn add_named_subtask(
        &self,
        name: impl Into<String>,
        task: tokio::task::JoinHandle<NodeSubtaskResult>,
    ) {
        let mut subtasks = self.subtasks.lock().await;
        subtasks.push(NodeSubtask {
            name: name.into(),
            handle: task,
        });
    }

    pub async fn stop(&self) {
        let mut subtasks = self.subtasks.lock().await;
        self.cancellation_token.cancel();

        while let Some(subtask) = subtasks.pop() {
            await_or_abort(subtask.handle).await;
        }
    }

    pub(crate) async fn collect_errors(&self) -> Vec<NodeTaskError> {
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
                Ok(Ok(())) => errors.push(NodeTaskError::StoppedUnexpectedly {
                    task_name: subtask.name,
                }),
                Ok(Err(error)) => errors.push(NodeTaskError::Failed {
                    task_name: subtask.name,
                    message: error.message,
                }),
                Err(join_error) if join_error.is_cancelled() => {}
                Err(join_error) if join_error.is_panic() => errors.push(NodeTaskError::Panicked {
                    task_name: subtask.name,
                    message: join_error.to_string(),
                }),
                Err(join_error) => errors.push(NodeTaskError::Failed {
                    task_name: subtask.name,
                    message: join_error.to_string(),
                }),
            }
        }

        errors
    }
}

async fn await_or_abort(mut handle: tokio::task::JoinHandle<NodeSubtaskResult>) {
    if tokio::time::timeout(Duration::from_millis(500), &mut handle).await.is_err() {
        handle.abort();
        let _ = handle.await;
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
        assert!(errors[0].to_string().contains("panic-task"));
    }

    #[tokio::test]
    async fn collects_explicit_subtask_errors() {
        let node_task = NodeTask::new();

        node_task
            .add_named_subtask(
                "fallible-task",
                tokio::spawn(async {
                    Err(NodeSubtaskFailure {
                        message: "database unavailable".to_string(),
                    })
                }),
            )
            .await;

        tokio::task::yield_now().await;
        let errors = node_task.collect_errors().await;

        assert_eq!(
            errors,
            vec![NodeTaskError::Failed {
                task_name: "fallible-task".to_string(),
                message: "database unavailable".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn stop_aborts_without_reporting_errors() {
        let node_task = NodeTask::new();

        node_task
            .add_named_subtask(
                "running-task",
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    Ok(())
                }),
            )
            .await;

        node_task.stop().await;

        assert!(node_task.collect_errors().await.is_empty());
    }

    #[tokio::test]
    async fn stop_requests_cooperative_cancellation() {
        let node_task = NodeTask::new();
        let token = node_task.cancellation_token();
        let observed_cancel = Arc::new(AtomicBool::new(false));
        let observed_cancel_for_task = observed_cancel.clone();

        node_task
            .add_named_subtask(
                "cooperative-task",
                tokio::spawn(async move {
                    token.cancelled().await;
                    observed_cancel_for_task.store(true, Ordering::SeqCst);
                    Ok(())
                }),
            )
            .await;

        node_task.stop().await;

        assert!(observed_cancel.load(Ordering::SeqCst));
    }
}
