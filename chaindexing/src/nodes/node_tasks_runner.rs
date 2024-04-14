use super::node_task::NodeTask;

#[async_trait::async_trait]
pub trait NodeTasksRunner {
    async fn run(&self) -> Vec<NodeTask>;
}
