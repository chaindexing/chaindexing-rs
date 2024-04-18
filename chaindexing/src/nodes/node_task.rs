use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, PartialEq, Debug)]
struct NodeSubTask(*const tokio::task::JoinHandle<()>);

unsafe impl Send for NodeSubTask {}

#[derive(Clone, Debug)]
pub struct NodeTask {
    subtasks: Arc<Mutex<Vec<NodeSubTask>>>,
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
    pub async fn add_subtask(&self, task: &tokio::task::JoinHandle<()>) {
        let mut subtasks = self.subtasks.lock().await;
        subtasks.push(NodeSubTask(task));
    }
    pub async fn stop(&self) {
        let subtasks = self.subtasks.lock().await;
        for subtask in subtasks.iter() {
            if let Some(subtask) = unsafe { (subtask).0.as_ref() } {
                subtask.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn adds_a_tokio_task() {
        let node_task = NodeTask::new();
        let subtask = tokio::spawn(async {});

        node_task.add_subtask(&subtask).await;

        let subtask = NodeSubTask(&subtask);

        let added_subtasks = node_task.subtasks.lock().await;

        assert_eq!(subtask, *added_subtasks.first().unwrap());
    }

    #[tokio::test]
    async fn adds_multiple_flattened_tokio_tasks() {
        let node_task = NodeTask::new();
        let subtasks = [
            tokio::spawn(async {}),
            tokio::spawn(async {}),
            tokio::spawn(async {}),
        ];

        for subtask in subtasks.iter() {
            node_task.add_subtask(subtask).await;
        }

        let subtasks: Vec<_> = subtasks.iter().map(|t| NodeSubTask(t)).collect();

        let added_subtasks = node_task.subtasks.lock().await;

        for (index, subtask) in subtasks.iter().enumerate() {
            assert_eq!(subtask, added_subtasks.get(index).unwrap());
        }
    }

    #[tokio::test]
    async fn adds_multiple_nested_subtasks() {
        let node_task = NodeTask::new();

        let subtask = tokio::spawn({
            let node_task = node_task.clone();

            async move {
                let subtask = tokio::spawn({
                    let node_task = node_task.clone();

                    async move {
                        let subtask = tokio::spawn({
                            let node_task = node_task.clone();

                            async move {
                                let subtask = tokio::spawn(async move {});
                                node_task.add_subtask(&subtask).await;
                                assert_is_added(&subtask, &node_task).await;
                            }
                        });
                        node_task.add_subtask(&subtask).await;
                        assert_is_added(&subtask, &node_task).await;
                    }
                });

                node_task.add_subtask(&subtask).await;

                assert_is_added(&subtask, &node_task).await;
            }
        });

        node_task.add_subtask(&subtask).await;
        assert_is_added(&subtask, &node_task).await;

        async fn assert_is_added(subtask: &tokio::task::JoinHandle<()>, node_task: &NodeTask) {
            let subtask = NodeSubTask(subtask);

            let added_subtasks = node_task.subtasks.lock().await;

            assert!(added_subtasks.iter().any(|added_subtask| *added_subtask == subtask));
        }
    }
}
