mod event;

pub use event::{Event, EventParam, PartialEvent};

use std::collections::HashMap;

use crate::{contracts, ChainId, Contract, ContractAddress};
use ethers::types::{Block, Log, TxHash, U64};

pub fn get<S: Send + Sync + Clone>(
    logs: &[Log],
    contracts: &[Contract<S>],
    contract_addresses: &[ContractAddress],
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Vec<Event> {
    let events_by_topics = contracts::group_events_by_topics(contracts);
    let contract_addresses_by_address =
        ContractAddress::group_contract_addresses_by_address_and_chain_id(contract_addresses);

    logs.iter()
        .filter_map(
            |log @ Log {
                 topics,
                 address,
                 block_number,
                 ..
             }| {
                let contract_address =
                    contract_addresses_by_address.get(&(*address, *chain_id)).unwrap();
                let block_number = block_number.as_ref()?;
                let block = blocks_by_number.get(block_number).filter(|block| {
                    log.block_hash
                        .zip(block.hash)
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
                    block.timestamp.as_u64() as i64,
                ))
            },
        )
        .collect()
}
