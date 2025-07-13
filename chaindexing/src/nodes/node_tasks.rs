use crate::OptimizationConfig;

use chrono::Utc;
use std::fmt::Debug;

use super::node::{self, Node};
use super::node_tasks_runner::NodeTasksRunner;
use super::NodeTask;

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

#[allow(dead_code)]
pub struct NodeTasks<'a> {
    current_node: &'a Node,
    state: NodeTasksState,
    tasks: Vec<NodeTask>,
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

    pub async fn orchestrate(
        &mut self,
        optimization_config: &Option<OptimizationConfig>,
        active_nodes: &[Node],
        tasks_runner: &impl NodeTasksRunner,
    ) {
        let leader_node = node::elect_leader(active_nodes);

        if self.current_node.is_leader(leader_node) {
            match self.state {
                NodeTasksState::Idle | NodeTasksState::Aborted => {
                    self.make_active(tasks_runner).await;
                }

                NodeTasksState::Active => {
                    if let Some(OptimizationConfig {
                        node_heartbeat,
                        start_after_in_secs,
                    }) = optimization_config
                    {
                        if node_heartbeat.is_stale().await
                            && self.started_n_seconds_ago(*start_after_in_secs)
                        {
                            self.make_inactive().await;
                        }
                    }
                }

                NodeTasksState::InActive => {
                    if let Some(OptimizationConfig { node_heartbeat, .. }) = optimization_config {
                        if node_heartbeat.is_recent().await {
                            self.make_active(tasks_runner).await;
                        }
                    }
                }
            }
        } else if self.state == NodeTasksState::Active {
            self.abort().await;
        }
    }

    async fn make_active(&mut self, tasks_runner: &impl NodeTasksRunner) {
        self.tasks = tasks_runner.run().await;
        self.state = NodeTasksState::Active;
    }
    async fn make_inactive(&mut self) {
        self.stop().await;
        self.state = NodeTasksState::InActive;
    }
    async fn abort(&mut self) {
        self.stop().await;
        self.state = NodeTasksState::Aborted;
    }
    async fn stop(&mut self) {
        for task in &self.tasks {
            task.stop().await;
        }
    }

    pub fn started_n_seconds_ago(&self, n_seconds: u64) -> bool {
        Self::now_in_secs() - self.started_at_in_secs >= n_seconds
    }

    fn now_in_secs() -> u64 {
        Utc::now().timestamp() as u64
    }
}
