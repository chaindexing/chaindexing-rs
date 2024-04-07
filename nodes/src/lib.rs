mod keep_node_active_request;
mod node;
mod node_tasks;

use crate::node::Node;

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
