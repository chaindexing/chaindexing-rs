use chrono::Utc;

use crate::Node;
use config::OptimizationConfig;

#[derive(PartialEq, Debug)]
enum NodeTasksState {
    /// Initial state of tasks are Idle.
    /// In this state, no NodeTask is running because nothing has happened yet.
    Idle,
    /// All NodeTasks are running when Active.
    Active,
    /// All NodeTasks are NOT running.
    /// However, when there is a recent KeepNodeActiveRequest, they get reactivated.
    InActive,
    /// All NodeTasks are NOT running.
    /// If there is a recent KeepNodeActiveRequest, it stays aborted.
    /// Only non-leader Nodes self-abort.
    Aborted,
}

type TaskProcess = tokio::task::JoinHandle<()>;

pub struct NodeTasks<'a> {
    current_node: &'a Node,
    state: NodeTasksState,
    tasks: Vec<TaskProcess>,
    started_at_in_secs: u64,
    /// Not used currently. In V2, We will populate NodeTasksErrors here
    pub errors: Vec<String>,
}

impl<'a> NodeTasks<'a> {
    pub fn new(current_node: &'a Node) -> Self {
        Self {
            current_node,
            state: NodeTasksState::Idle,
            started_at_in_secs: Self::now_in_secs(),
            tasks: vec![],
            errors: vec![],
        }
    }

    pub async fn maybe_keep_alive<F>(
        &mut self,
        optimization_config: &OptimizationConfig,
        active_nodes: &Vec<Node>,
        start_tasks: &F,
    ) where
        F: Fn() -> Vec<TaskProcess>,
    {
        let leader_node = super::elect_leader(&active_nodes);

        if self.current_node.is_leader(&leader_node) {
            match self.state {
                NodeTasksState::Idle | NodeTasksState::Aborted => self.make_active(config),

                NodeTasksState::Active => {
                    if let Some(OptimizationConfig {
                        keep_node_active_request,
                        optimize_after_in_secs,
                    }) = &optimization_config
                    {
                        if keep_node_active_request.is_stale().await
                            && self.started_n_seconds_ago(*optimize_after_in_secs)
                        {
                            self.make_inactive()
                        }
                    }
                }

                NodeTasksState::InActive => {
                    if let Some(OptimizationConfig {
                        keep_node_active_request,
                        ..
                    }) = &optimization_config
                    {
                        if keep_node_active_request.is_recent().await {
                            self.make_active(config)
                        }
                    }
                }
            }
        } else {
            if self.state == NodeTasksState::Active {
                self.abort();
            }
        }
    }

    fn make_active<F>(&mut self, start_tasks: &F)
    where
        F: Fn() -> Vec<TaskProcess>,
    {
        self.start(start_tasks);
        self.state = NodeTasksState::Active;
    }
    fn make_inactive(&mut self) {
        self.stop();
        self.state = NodeTasksState::InActive;
    }
    fn abort(&mut self) {
        self.stop();
        self.state = NodeTasksState::Aborted;
    }

    fn start<F>(&mut self, start_tasks: &F)
    where
        F: Fn() -> Vec<TaskProcess>,
    {
        self.tasks = start_tasks();
    }
    fn stop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }

    pub fn started_n_seconds_ago(&self, n_seconds: u64) -> bool {
        Self::now_in_secs() - self.started_at_in_secs >= n_seconds
    }

    fn now_in_secs() -> u64 {
        Utc::now().timestamp() as u64
    }
}
