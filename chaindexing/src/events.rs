mod event;

pub use event::{Event, EventParam, PartialEvent};

use std::collections::HashMap;

use crate::contracts::Contracts;
use ethers::types::{Block, Log, TxHash, U64};

use crate::Contract;

pub fn get<S: Send + Sync + Clone>(
    logs: &[Log],
    contracts: &Vec<Contract<S>>,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Vec<Event> {
    let events_by_topics = Contracts::group_events_by_topics(contracts);
    let contract_addresses_by_address =
        Contracts::get_all_contract_addresses_grouped_by_address(contracts);

    logs.iter()
        .map(
            |log @ Log {
                 topics,
                 address,
                 block_number,
                 ..
             }| {
                let contract_address = contract_addresses_by_address.get(address).unwrap();
                let block = blocks_by_number.get(&block_number.unwrap()).unwrap();

                Event::new(
                    log,
                    events_by_topics.get(&topics[0]).unwrap(),
                    contract_address,
                    block.timestamp.as_u64() as i64,
                )
            },
        )
        .collect()
}
