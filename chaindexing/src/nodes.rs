use diesel::{prelude::Insertable, Queryable};
use serde::Deserialize;

use crate::node_task::NodeTask;
use crate::OptimizationConfig;

use super::diesel::schema::chaindexing_nodes;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Insertable, Queryable)]
#[diesel(table_name = chaindexing_nodes)]
pub struct Node {
    pub id: i32,
    last_active_at: i64,
    inserted_at: i64,
}

impl Node {
    pub fn get_min_active_at_in_secs(node_election_rate_ms: u64) -> i64 {
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Not active if not kept active at least 2 elections away
        (now_ms - (2 * node_election_rate_ms) as i64) / 1_000
    }

    fn is_leader(&self, leader: &Node) -> bool {
        self.id == leader.id
    }
}

use chrono::Utc;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;

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

#[derive(Clone, Debug)]
pub struct KeepNodeActiveRequest {
    /// Both in milliseconds
    last_refreshed_at_and_active_grace_period: Arc<Mutex<(u64, u32)>>,
}

impl KeepNodeActiveRequest {
    /// * `active_grace_period_ms` - how long should the Node wait
    /// till it goes inactive
    pub fn new(active_grace_period_ms: u32) -> Self {
        Self {
            last_refreshed_at_and_active_grace_period: Arc::new(Mutex::new((
                Self::now(),
                active_grace_period_ms,
            ))),
        }
    }
    pub async fn refresh(&self) {
        let mut last_refreshed_at_and_active_grace_period =
            self.last_refreshed_at_and_active_grace_period.lock().await;
        *last_refreshed_at_and_active_grace_period =
            (Self::now(), last_refreshed_at_and_active_grace_period.1);
    }

    fn now() -> u64 {
        Utc::now().timestamp_millis() as u64
    }

    async fn is_stale(&self) -> bool {
        !self.is_recent().await
    }
    async fn is_recent(&self) -> bool {
        let (last_refreshed_at, active_grace_period) =
            *self.last_refreshed_at_and_active_grace_period.lock().await;
        let min_last_refreshed_at = Self::now() - (active_grace_period as u64);

        last_refreshed_at > min_last_refreshed_at
    }
}

#[async_trait::async_trait]
pub trait NodeTasksRunner {
    async fn run(&self) -> Vec<NodeTask>;
}

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
        start_tasks: &impl NodeTasksRunner,
    ) {
        let leader_node = elect_leader(active_nodes);

        if self.current_node.is_leader(leader_node) {
            match self.state {
                NodeTasksState::Idle | NodeTasksState::Aborted => {
                    self.make_active(start_tasks).await;
                }

                NodeTasksState::Active => {
                    if let Some(OptimizationConfig {
                        keep_node_active_request,
                        optimize_after_in_secs,
                    }) = optimization_config
                    {
                        if keep_node_active_request.is_stale().await
                            && self.started_n_seconds_ago(*optimize_after_in_secs)
                        {
                            self.make_inactive().await;
                        }
                    }
                }

                NodeTasksState::InActive => {
                    if let Some(OptimizationConfig {
                        keep_node_active_request,
                        ..
                    }) = optimization_config
                    {
                        if keep_node_active_request.is_recent().await {
                            self.make_active(start_tasks).await;
                        }
                    }
                }
            }
        } else if self.state == NodeTasksState::Active {
            self.abort().await;
        }
    }

    async fn make_active(&mut self, start_tasks: &impl NodeTasksRunner) {
        self.tasks = start_tasks.run().await;
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

pub const DEFAULT_MAX_CONCURRENT_NODE_COUNT: u16 = 50;

fn elect_leader(nodes: &[Node]) -> &Node {
    let mut nodes_iter = nodes.iter();
    let mut leader: Option<&Node> = nodes_iter.next();

    for node in nodes_iter {
        if node.inserted_at > leader.unwrap().inserted_at {
            leader = Some(node);
        }
    }

    leader.unwrap()
}
