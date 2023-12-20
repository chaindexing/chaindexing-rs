use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::diesels::schema::chaindexing_contract_addresses;
use crate::{ContractStateMigrations, EventHandler};
use diesel::{Identifiable, Insertable, Queryable};

use ethers::{
    abi::{Address, Event, HumanReadableParser},
    prelude::Chain,
    types::H256,
};

pub type ContractEventTopic = H256;

#[derive(Debug, Clone)]
pub struct ContractEvent {
    pub abi: String,
    pub value: Event,
}

impl ContractEvent {
    pub fn new(abi: &str) -> Self {
        Self {
            abi: abi.to_string(),
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
    pub state_migrations: Vec<Arc<dyn ContractStateMigrations>>,
}

impl Contract {
    pub fn new(name: &str) -> Self {
        Self {
            addresses: vec![],
            state_migrations: vec![],
            name: name.to_string(),
            event_handlers: HashMap::new(),
        }
    }

    pub fn add_address(&self, address: &str, chain: &Chain, start_block_number: i64) -> Self {
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
        mut self,
        event_abi: EventAbi,
        event_handler: impl EventHandler + 'static,
    ) -> Self {
        self.event_handlers.insert(event_abi, Arc::new(event_handler));

        self
    }

    pub fn add_state_migrations(
        mut self,
        state_migration: impl ContractStateMigrations + 'static,
    ) -> Self {
        self.state_migrations.push(Arc::new(state_migration));

        self
    }

    pub fn get_event_abis(&self) -> Vec<EventAbi> {
        self.event_handlers.clone().into_keys().collect()
    }

    pub fn get_event_topics(&self) -> Vec<ContractEventTopic> {
        self.get_event_abis()
            .iter()
            .map(|abi| HumanReadableParser::parse_event(abi).unwrap().signature())
            .collect()
    }

    pub fn build_events(&self) -> Vec<ContractEvent> {
        self.get_event_abis().iter().map(|abi| ContractEvent::new(abi)).collect()
    }
}

pub struct Contracts;

impl Contracts {
    pub fn get_state_migrations(contracts: &[Contract]) -> Vec<Arc<dyn ContractStateMigrations>> {
        contracts.iter().flat_map(|c| c.state_migrations.clone()).collect()
    }

    pub fn get_all_event_handlers_by_event_abi(
        contracts: &[Contract],
    ) -> HashMap<EventAbi, Arc<dyn EventHandler>> {
        contracts.iter().fold(
            HashMap::new(),
            |mut event_handlers_by_event_abi, contract| {
                contract.event_handlers.iter().for_each(|(event_abi, event_handler)| {
                    event_handlers_by_event_abi.insert(event_abi, event_handler.clone());
                });

                event_handlers_by_event_abi
            },
        )
    }

    pub fn group_event_topics_by_names(
        contracts: &[Contract],
    ) -> HashMap<String, Vec<ContractEventTopic>> {
        contracts.iter().fold(HashMap::new(), |mut topics_by_contract_name, contract| {
            topics_by_contract_name.insert(contract.name.clone(), contract.get_event_topics());

            topics_by_contract_name
        })
    }

    pub fn group_events_by_topics(
        contracts: &[Contract],
    ) -> HashMap<ContractEventTopic, ContractEvent> {
        contracts
            .iter()
            .flat_map(|c| c.build_events())
            .map(|e| (e.value.signature(), e))
            .collect()
    }

    pub fn get_all_contract_addresses_grouped_by_address(
        contracts: &[Contract],
    ) -> HashMap<Address, &UnsavedContractAddress> {
        contracts.iter().fold(HashMap::new(), |mut contracts_by_addresses, contract| {
            contract.addresses.iter().for_each(
                |contract_address @ UnsavedContractAddress { address, .. }| {
                    contracts_by_addresses.insert(
                        Address::from_str(address.as_str()).unwrap(),
                        contract_address,
                    );
                },
            );

            contracts_by_addresses
        })
    }
}

#[derive(Debug, Clone, PartialEq, Insertable)]
#[diesel(table_name = chaindexing_contract_addresses)]
pub struct UnsavedContractAddress {
    pub contract_name: String,
    address: String,
    pub chain_id: i32,
    start_block_number: i64,
    next_block_number_to_ingest_from: i64,
    next_block_number_to_handle_from: i64,
}

impl UnsavedContractAddress {
    pub fn new(contract_name: &str, address: &str, chain: &Chain, start_block_number: i64) -> Self {
        UnsavedContractAddress {
            contract_name: contract_name.to_string(),
            address: address.to_lowercase().to_string(),
            chain_id: *chain as i32,
            start_block_number,
            next_block_number_to_ingest_from: start_block_number,
            next_block_number_to_handle_from: start_block_number,
        }
    }
}

pub struct ContractAddressID(pub i32);

impl ContractAddressID {
    pub fn value(self) -> i32 {
        self.0
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
    pub next_block_number_to_ingest_from: i64,
    pub next_block_number_to_handle_from: i64,
    pub start_block_number: i64,
    pub address: String,
    pub contract_name: String,
}

impl ContractAddress {
    pub fn id(&self) -> ContractAddressID {
        ContractAddressID(self.id)
    }
}
