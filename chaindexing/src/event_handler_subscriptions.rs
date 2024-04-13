use serde::Deserialize;

use crate::ChainId;

#[derive(Debug, Clone, Deserialize)]
pub struct EventHandlerSubscription {
    pub chain_id: u64,
    pub next_block_number_to_handle_from: u64,
}

impl EventHandlerSubscription {
    pub fn new(chain_id: &ChainId, next_block_number_to_handle_from: u64) -> Self {
        Self {
            chain_id: *chain_id as u64,
            next_block_number_to_handle_from,
        }
    }
}
