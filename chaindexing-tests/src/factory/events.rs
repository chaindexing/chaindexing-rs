use std::collections::HashMap;

use chaindexing::{Contract, Event, Events};
use ethers::types::Block;

use super::{transfer_log, BAYC_CONTRACT_ADDRESS};

pub fn transfer_event_with_contract(contract: Contract) -> Event {
    let contract_address = BAYC_CONTRACT_ADDRESS;
    let transfer_log = transfer_log(contract_address);
    let blocks_by_number = HashMap::from([(
        transfer_log.block_number.clone().unwrap(),
        Block {
            ..Default::default()
        },
    )]);
    Events::new(&vec![transfer_log], &vec![contract], &blocks_by_number)
        .first()
        .cloned()
        .unwrap()
}
