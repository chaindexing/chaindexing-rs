mod node;
mod node_heartbeat;
mod node_task;
mod node_tasks;
mod node_tasks_runner;

pub use node::Node;
pub use node_heartbeat::NodeHeartbeat;
pub use node_task::NodeTask;
pub use node_tasks::NodeTasks;
pub use node_tasks_runner::NodeTasksRunner;

use crate::{handlers, ingester, Config};

use std::fmt::Debug;

pub const DEFAULT_MAX_CONCURRENT_NODE_COUNT: u16 = 50;

pub fn get_tasks_runner<S: Sync + Send + Debug + Clone + 'static>(
    config: &Config<S>,
) -> impl NodeTasksRunner + '_ {
    struct ChaindexingNodeTasksRunner<'a, S: Send + Sync + Clone + Debug + 'static> {
        config: &'a Config<S>,
    }
    #[async_trait::async_trait]
    impl<'a, S: Send + Sync + Clone + Debug + 'static> NodeTasksRunner
        for ChaindexingNodeTasksRunner<'a, S>
    {
        async fn run(&self) -> Vec<NodeTask> {
            let ingester = ingester::start(self.config).await;
            let handlers = handlers::start(self.config).await;

            vec![ingester, handlers]
        }
    }
    ChaindexingNodeTasksRunner { config }
}
