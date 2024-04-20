use super::node_task::NodeTask;

#[crate::augmenting_std::async_trait]
pub trait NodeTasksRunner {
    async fn run(&self) -> Vec<NodeTask>;
}
