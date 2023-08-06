use std::{collections::HashMap, sync::Arc};

use crate::{diesel::schema::chaindexing_contract_addresses, event_handlers::EventHandler};
use diesel::{Identifiable, Insertable, Queryable};

use ethers::{
    abi::{Event, HumanReadableParser},
    prelude::Chain,
    types::H256,
};

pub type ContractEventTopic = H256;

#[derive(Debug, Clone)]
pub struct ContractEvent {
    pub abi: String,
    pub contract_name: String,
    pub value: Event,
}

impl ContractEvent {
    pub fn new(abi: &str, contract_name: &str) -> Self {
        Self {
            abi: abi.to_string(),
            contract_name: contract_name.to_string(),
            value: HumanReadableParser::parse_event(abi).unwrap(),
        }
    }
}

type EventAbi = &'static str;

#[derive(Clone)]
pub struct Contract {
    pub addresses: Vec<UnsavedContractAddress>,
    pub name: String,
    pub event_handlers: HashMap<EventAbi, Arc<dyn EventHandler>>,
}

impl Contract {
    pub fn new(name: &str) -> Self {
        Self {
            addresses: vec![],
            name: name.to_string(),
            event_handlers: HashMap::new(),
        }
    }

    pub fn add_address(&self, address: &str, chain: &Chain, start_block_number: u32) -> Self {
        let mut addresses = self.addresses.clone();

        addresses.push(UnsavedContractAddress::new(
            &self.name,
            address,
            chain,
            start_block_number,
        ));

        Self {
            addresses,
            ..self.clone()
        }
    }

    pub fn add_event(
        &self,
        event_abi: EventAbi,
        event_handler: impl EventHandler + 'static,
    ) -> Self {
        let mut event_handlers = self.event_handlers.clone();

        event_handlers.insert(event_abi, Arc::new(event_handler));

        Self {
            event_handlers,
            ..self.clone()
        }
    }

    pub fn get_event_abis(&self) -> Vec<EventAbi> {
        self.event_handlers.clone().into_keys().collect()
    }

    pub fn get_event_topics(&self) -> Vec<ContractEventTopic> {
        self.get_event_abis()
            .into_iter()
            .map(|abi| HumanReadableParser::parse_event(abi).unwrap().signature())
            .collect()
    }

    pub fn get_events(&self) -> Vec<ContractEvent> {
        self.get_event_abis()
            .into_iter()
            .map(|abi| ContractEvent::new(abi, &self.name))
            .collect()
    }
}

pub struct Contracts;

impl Contracts {
    pub fn group_event_topics_by_names(
        contracts: &Vec<Contract>,
    ) -> HashMap<String, Vec<ContractEventTopic>> {
        contracts
            .into_iter()
            .fold(HashMap::new(), |mut topics_by_contract_name, contract| {
                // embrace mutability because Rust helps avoid bugs
                topics_by_contract_name.insert(contract.name.clone(), contract.get_event_topics());

                topics_by_contract_name
            })
    }

    pub fn group_events_by_topics<'a>(
        contracts: &'a Vec<Contract>,
    ) -> HashMap<ContractEventTopic, ContractEvent> {
        contracts
            .iter()
            .flat_map(|c| c.get_events())
            .map(|e| (e.value.signature(), e))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Insertable)]
#[diesel(table_name = chaindexing_contract_addresses)]
pub struct UnsavedContractAddress {
    contract_name: String,
    address: String,
    chain_id: i32,
    start_block_number: i32,
    last_ingested_block_number: i32,
}

impl UnsavedContractAddress {
    pub fn new(contract_name: &str, address: &str, chain: &Chain, start_block_number: u32) -> Self {
        UnsavedContractAddress {
            contract_name: contract_name.to_string(),
            address: address.to_string(),
            chain_id: *chain as i32,
            start_block_number: start_block_number as i32,
            last_ingested_block_number: start_block_number as i32,
        }
    }
}

/// Answers: Where is a given EVM contract located
/// N/B: The order has to match ./schema.rs to stop diesel from mixing up fields...lol
#[derive(Debug, Clone, PartialEq, Queryable, Identifiable)]
#[diesel(table_name = chaindexing_contract_addresses)]
#[diesel(primary_key(id))]
pub struct ContractAddress {
    pub id: i32,
    chain_id: i32,
    // TODO: Update to i64
    pub last_ingested_block_number: i32,
    // TODO: Update to i64
    start_block_number: i32,
    pub address: String,
    pub contract_name: String,
}
