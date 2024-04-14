use std::collections::HashMap;

use serde::Deserialize;

use crate::ChainId;

#[derive(Debug, Clone, Deserialize)]
pub struct HandlerSubscription {
    pub chain_id: u64,
    pub next_block_number_to_handle_from: u64,
    pub next_block_number_for_side_effect: u64,
}

impl HandlerSubscription {
    pub fn new(chain_id: &ChainId, next_block_number_to_handle_from: u64) -> Self {
        Self {
            chain_id: *chain_id as u64,
            next_block_number_to_handle_from,
            next_block_number_for_side_effect: next_block_number_to_handle_from,
        }
    }
}

pub fn group_by_chain_id(
    subscriptions: &[HandlerSubscription],
) -> HashMap<u64, &HandlerSubscription> {
    subscriptions.iter().fold(
        HashMap::new(),
        |mut subscriptions_by_chain_id, subscription| {
            subscriptions_by_chain_id.insert(subscription.chain_id, subscription);
            subscriptions_by_chain_id
        },
    )
}
