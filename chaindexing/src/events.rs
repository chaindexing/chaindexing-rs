mod event;

pub use event::{Event, EventParam, PartialEvent};

use std::collections::HashMap;

use crate::{contracts, ChainId, Contract, ContractAddress};
use alloy::rpc::types::{Block, Log};

pub fn get<S: Send + Sync + Clone>(
    logs: &[Log],
    contracts: &[Contract<S>],
    contract_addresses: &[ContractAddress],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<u64, Block>,
) -> Vec<Event> {
    let events_by_topics = contracts::group_events_by_topics(contracts);
    let contract_addresses_by_address =
        ContractAddress::group_contract_addresses_by_address_and_chain_id(contract_addresses);

    logs.iter()
        .filter_map(|log| {
            let topics = log.topics();
            let address = log.address();
            let block_number = log.block_number?;
            let contract_address =
                contract_addresses_by_address.get(&(address, *chain_id)).unwrap();
            let block = blocks_by_number.get(&block_number).filter(|block| {
                log.block_hash
                    .map(|log_hash| (log_hash, block.header.hash))
                    .map(|(log_hash, block_hash)| log_hash == block_hash)
                    .unwrap_or(false)
            })?;
            let contract_event_key =
                contracts::contract_event_key(&contract_address.contract_name, topics[0]);

            Some(Event::new(
                log,
                events_by_topics.get(&contract_event_key).unwrap(),
                chain_id,
                &contract_address.contract_name,
                block.header.inner.timestamp as i64,
            ))
        })
        .collect()
}
