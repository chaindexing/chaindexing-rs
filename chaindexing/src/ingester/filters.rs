use std::collections::HashMap;

use ethers::types::{Address, Filter as EthersFilter};
use std::cmp::min;

use crate::chain_reorg::Execution;
use crate::contracts;
use crate::contracts::Contract;
use crate::contracts::ContractEventTopic;
use crate::ContractAddress;

pub fn get<S: Send + Sync + Clone>(
    contract_addresses: &[ContractAddress],
    contracts: &[Contract<S>],
    current_block_number: u64,
    blocks_per_batch: u64,
    execution: &Execution,
) -> Vec<Filter> {
    let topics_by_contract_name = contracts::group_event_topics_by_names(contracts);

    contract_addresses
        .iter()
        .map_while(|contract_address| {
            topics_by_contract_name.get(contract_address.contract_name.as_str()).and_then(
                |topics| {
                    Filter::maybe_new(
                        contract_address,
                        topics,
                        current_block_number,
                        blocks_per_batch,
                        execution,
                    )
                },
            )
        })
        .collect()
}

pub fn group_by_contract_address_id(filters: &[Filter]) -> HashMap<i64, Vec<Filter>> {
    let empty_filter_group = vec![];

    filters.iter().fold(
        HashMap::new(),
        |mut filters_by_contract_address_id, filter| {
            let mut filter_group = filters_by_contract_address_id
                .get(&filter.contract_address_id)
                .unwrap_or(&empty_filter_group)
                .to_vec();

            filter_group.push(filter.clone());

            filters_by_contract_address_id.insert(filter.contract_address_id, filter_group);

            filters_by_contract_address_id
        },
    )
}

pub fn get_latest(filters: &Vec<Filter>) -> Option<Filter> {
    let mut filters = filters.to_owned();
    filters.sort_by_key(|f| f.value.get_to_block());

    filters.last().cloned()
}

#[derive(Clone, Debug)]
pub struct Filter {
    pub contract_address_id: i64,
    pub address: String,
    pub value: EthersFilter,
}

impl Filter {
    fn maybe_new(
        contract_address: &ContractAddress,
        topics: &[ContractEventTopic],
        current_block_number: u64,
        blocks_per_batch: u64,
        execution: &Execution,
    ) -> Option<Filter> {
        let ContractAddress {
            id: contract_address_id,
            next_block_number_to_ingest_from,
            start_block_number,
            address,
            ..
        } = contract_address;

        let next_block_number_to_ingest_from = *next_block_number_to_ingest_from as u64;

        match execution {
            Execution::Main => Some((
                next_block_number_to_ingest_from,
                min(
                    next_block_number_to_ingest_from + blocks_per_batch,
                    current_block_number,
                ),
            )),
            Execution::Confirmation(min_confirmation_count) => {
                // TODO: Move logic to higher level
                if min_confirmation_count.is_in_confirmation_window(
                    next_block_number_to_ingest_from,
                    current_block_number,
                ) {
                    Some((
                        min_confirmation_count.deduct_from(
                            next_block_number_to_ingest_from,
                            *start_block_number as u64,
                        ),
                        next_block_number_to_ingest_from + blocks_per_batch,
                    ))
                } else {
                    None
                }
            }
        }
        .map(|(from_block_number, to_block_number)| Filter {
            contract_address_id: *contract_address_id,
            address: address.to_string(),
            value: EthersFilter::new()
                .address(address.parse::<Address>().unwrap())
                .topic0(topics.to_vec())
                .from_block(from_block_number)
                .to_block(to_block_number),
        })
    }
}
