use diesel::{prelude::Insertable, Queryable};
use serde::Deserialize;

use crate::{Migratable, RepoMigrations};

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
    pub const ELECTION_RATE_SECS: u64 = 60;

    pub fn get_min_active_at() -> i64 {
        let now = chrono::Utc::now().timestamp();

        // Not active if not kept active at least 2 elections away
        now - (Node::ELECTION_RATE_SECS * 2) as i64
    }

    fn is_leader(&self, active_nodes: &Vec<Node>) -> bool {
        let leader_node = elect_leader(&active_nodes);

        self.id == leader_node.id
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

use chrono::{DateTime, Utc};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(PartialEq)]
enum NodeTasksState {
    Idle,
    Active,
    Aborted,
}

pub struct NodeTasks<'a> {
    current_node: &'a Node,
    state: NodeTasksState,
    pub last_keep_active_at: Arc<Mutex<DateTime<Utc>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// Not used currently. In V2, We will populate NodeTasksErrors here
    pub errors: Vec<String>,
}

impl<'a> NodeTasks<'a> {
    pub fn new(current_node: &'a Node) -> Self {
        Self {
            current_node,
            state: NodeTasksState::Idle,
            last_keep_active_at: Arc::new(Mutex::new(Utc::now())),
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

        if self.current_node.is_leader(&active_nodes) {
            match self.state {
                NodeTasksState::Idle | NodeTasksState::Aborted => self.start(config),

                NodeTasksState::Active => {
                    ChaindexingRepo::keep_node_active(conn, &self.current_node).await
                }
            }
        } else {
            if self.state == NodeTasksState::Active {
                self.abort();
            }
        }
    }

    fn start<S: Send + Sync + Clone + Debug + 'static>(&mut self, config: &Config<S>) {
        let event_ingester = EventsIngester::start(config);
        let event_handlers = EventHandlers::start(config);

        self.tasks = vec![event_ingester, event_handlers];
        self.state = NodeTasksState::Active;
    }
    fn abort(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.state = NodeTasksState::Aborted;
    }
}

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
