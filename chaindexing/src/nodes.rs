use diesel::{prelude::Insertable, Queryable};
use serde::Deserialize;

use crate::{Migratable, OptimizationConfig, RepoMigrations};

use super::diesels::schema::chaindexing_nodes;
use super::{ChaindexingRepo, ChaindexingRepoConn, ChaindexingRepoRawQueryClient, Repo};

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Insertable, Queryable)]
#[diesel(table_name = chaindexing_nodes)]
pub struct Node {
    pub id: i32,
    last_active_at: i64,
    inserted_at: i64,
}

impl Node {
    pub const ELECTION_RATE_SECS: u64 = 15;

    pub fn get_min_active_at() -> i64 {
        let now = chrono::Utc::now().timestamp();

        // Not active if not kept active at least 2 elections away
        now - (Node::ELECTION_RATE_SECS * 2) as i64
    }

    fn is_leader(&self, leader: &Node) -> bool {
        self.id == leader.id
    }

    pub async fn create<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        client: &ChaindexingRepoRawQueryClient,
    ) -> Node {
        ChaindexingRepo::migrate(client, ChaindexingRepo::create_nodes_migration().to_vec()).await;
        ChaindexingRepo::create_node(conn).await
    }
}

use super::Config;
use super::{EventHandlers, EventsIngester};

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

#[derive(Clone)]
pub struct KeepNodeActiveRequest {
    /// Both in milliseconds
    last_refreshed_at_and_active_grace_period: Arc<Mutex<(u64, u32)>>,
}

impl KeepNodeActiveRequest {
    pub fn new(active_grace_period: u32) -> Self {
        Self {
            last_refreshed_at_and_active_grace_period: Arc::new(Mutex::new((
                Self::now(),
                active_grace_period,
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

pub struct NodeTasks<'a> {
    current_node: &'a Node,
    state: NodeTasksState,
    tasks: Vec<tokio::task::JoinHandle<()>>,
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

    pub async fn orchestrate<'b, S: Send + Sync + Clone + Debug + 'static>(
        &mut self,
        config: &Config<S>,
        conn: &mut ChaindexingRepoConn<'b>,
    ) {
        let active_nodes = ChaindexingRepo::get_active_nodes(conn).await;
        let leader_node = elect_leader(&active_nodes);

        if self.current_node.is_leader(&leader_node) {
            match self.state {
                NodeTasksState::Idle | NodeTasksState::Aborted => self.make_active(config),

                NodeTasksState::Active => {
                    if let Some(OptimizationConfig {
                        keep_node_active_request,
                        optimize_after_in_secs,
                    }) = &config.optimization_config
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
                    }) = &config.optimization_config
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

        ChaindexingRepo::keep_node_active(conn, &self.current_node).await;
    }

    fn make_active<S: Send + Sync + Clone + Debug + 'static>(&mut self, config: &Config<S>) {
        self.start(config);
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

    fn start<S: Send + Sync + Clone + Debug + 'static>(&mut self, config: &Config<S>) {
        let event_ingester = EventsIngester::start(config);
        let event_handlers = EventHandlers::start(config);

        self.tasks = vec![event_ingester, event_handlers];
    }
    fn stop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }

    fn started_n_seconds_ago(&self, n_seconds: u64) -> bool {
        Self::now_in_secs() - self.started_at_in_secs >= n_seconds
    }

    fn now_in_secs() -> u64 {
        Utc::now().timestamp() as u64
    }
}

pub const DEFAULT_MAX_CONCURRENT_NODE_COUNT: u16 = 50;

fn elect_leader<'a>(nodes: &'a Vec<Node>) -> &'a Node {
    let mut nodes_iter = nodes.iter();
    let mut leader: Option<&Node> = nodes_iter.next();

    while let Some(node) = nodes_iter.next() {
        if node.inserted_at > leader.unwrap().inserted_at {
            leader = Some(node);
        }
    }

    leader.unwrap()
}
