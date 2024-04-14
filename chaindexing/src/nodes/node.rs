use diesel::{prelude::Insertable, Queryable};
use serde::Deserialize;
use std::fmt::Debug;

use crate::diesel::schema::chaindexing_nodes;

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

    pub fn is_leader(&self, leader: &Node) -> bool {
        self.id == leader.id
    }
}

pub fn elect_leader(nodes: &[Node]) -> &Node {
    let mut nodes_iter = nodes.iter();
    let mut leader: Option<&Node> = nodes_iter.next();

    for node in nodes_iter {
        if node.inserted_at > leader.unwrap().inserted_at {
            leader = Some(node);
        }
    }

    leader.unwrap()
}
