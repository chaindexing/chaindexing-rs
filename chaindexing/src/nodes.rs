/// Nodes are Chaindexing instances in a distributed environment
/// Responsible for managing the core tasks of each node including
/// keeping each node alive programmatically, resolving
/// indexing configuration conflicts, etc.
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

pub const DEFAULT_MAX_CONCURRENT_NODE_COUNT: u16 = 50;
