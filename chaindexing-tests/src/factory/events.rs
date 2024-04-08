use std::collections::HashMap;

use chaindexing::{events, Contract, Event};
use ethers::types::Block;

use super::{transfer_log, BAYC_CONTRACT_ADDRESS};

pub fn transfer_event_with_contract(contract: Contract<()>) -> Event {
    let contract_address = BAYC_CONTRACT_ADDRESS;
    let transfer_log = transfer_log(contract_address);
    let blocks_by_number = HashMap::from([(
        transfer_log.block_number.unwrap(),
        Block {
            ..Default::default()
        },
    )]);
    events::get(&[transfer_log], &[contract], &blocks_by_number)
        .first()
        .cloned()
        .unwrap()
}
