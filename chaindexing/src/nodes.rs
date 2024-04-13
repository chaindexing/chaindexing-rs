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

use crate::{event_handlers, events_ingester, Config};

use std::fmt::Debug;

pub const DEFAULT_MAX_CONCURRENT_NODE_COUNT: u16 = 50;

pub fn get_tasks_runner<'a, S: Sync + Send + Debug + Clone + 'static>(
    config: &'a Config<S>,
) -> impl NodeTasksRunner + 'a {
    struct ChaindexingNodeTasksRunner<'a, S: Send + Sync + Clone + Debug + 'static> {
        config: &'a Config<S>,
    }
    #[async_trait::async_trait]
    impl<'a, S: Send + Sync + Clone + Debug + 'static> NodeTasksRunner
        for ChaindexingNodeTasksRunner<'a, S>
    {
        async fn run(&self) -> Vec<NodeTask> {
            let events_ingester = events_ingester::start(self.config).await;
            let event_handlers = event_handlers::start(self.config).await;

            vec![events_ingester, event_handlers]
        }
    }
    ChaindexingNodeTasksRunner { config: &config }
}
