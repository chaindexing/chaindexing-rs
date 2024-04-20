use std::fmt::Debug;
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::diesel::schema::chaindexing_contract_addresses;
use crate::handlers::PureHandler;
use crate::states::StateMigrations;
use crate::ChainId;
use crate::{EventHandler, SideEffectHandler};
use diesel::{Identifiable, Insertable, Queryable};

use ethers::types::U64;
use ethers::{
    abi::{Address, Event, HumanReadableParser},
    types::H256,
};
use serde::Deserialize;

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

/// Human Readable ABI defined for ingesting events.
/// For example, `event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)`
pub type EventAbi = &'static str;

/// Represents the template/specification/interface for a given contract.
#[derive(Clone)]
pub struct Contract<S: Send + Sync + Clone> {
    pub addresses: Vec<UnsavedContractAddress>,
    pub name: String,
    pub pure_handlers: HashMap<EventAbi, Arc<dyn PureHandler>>,
    pub side_effect_handlers: HashMap<EventAbi, Arc<dyn SideEffectHandler<SharedState = S>>>,
    pub state_migrations: Vec<Arc<dyn StateMigrations>>,
}

impl<S: Send + Sync + Clone> Contract<S> {
    /// Builds the contract's template/spec/interface.
    ///
    ///
    /// # Example
    /// ```
    /// use chaindexing::Contract;
    ///
    /// Contract::<()>::new("ERC20");
    /// ```
    pub fn new(name: &str) -> Self {
        Self {
            addresses: vec![],
            state_migrations: vec![],
            name: name.to_string(),
            pure_handlers: HashMap::new(),
            side_effect_handlers: HashMap::new(),
        }
    }

    /// Adds a contract address to a contract
    pub fn add_address(
        mut self,
        address: &str,
        chain_id: &ChainId,
        start_block_number: u64,
    ) -> Self {
        self.addresses.push(UnsavedContractAddress::new(
            &self.name,
            address,
            chain_id,
            start_block_number,
        ));

        self
    }

    /// Adds an event handler
    pub fn add_event_handler(mut self, handler: impl EventHandler + 'static) -> Self {
        self.pure_handlers.insert(handler.abi(), Arc::new(handler));

        self
    }

    /// Adds a side-effect handler
    pub fn add_side_effect_handler(
        mut self,
        handler: impl SideEffectHandler<SharedState = S> + 'static,
    ) -> Self {
        self.side_effect_handlers.insert(handler.abi(), Arc::new(handler));

        self
    }

    /// Adds state migrations for the contract states being indexed
    pub fn add_state_migrations(mut self, state_migration: impl StateMigrations + 'static) -> Self {
        self.state_migrations.push(Arc::new(state_migration));

        self
    }

    pub(crate) fn get_event_abis(&self) -> Vec<EventAbi> {
        let mut event_abis: Vec<_> = self.pure_handlers.clone().into_keys().collect();
        let side_effect_abis: Vec<_> = self.pure_handlers.clone().into_keys().collect();

        event_abis.extend(side_effect_abis);
        event_abis.dedup();

        event_abis
    }

    pub(crate) fn get_event_topics(&self) -> Vec<ContractEventTopic> {
        self.get_event_abis()
            .iter()
            .map(|abi| HumanReadableParser::parse_event(abi).unwrap().signature())
            .collect()
    }

    pub(crate) fn build_events(&self) -> Vec<ContractEvent> {
        self.get_event_abis().iter().map(|abi| ContractEvent::new(abi)).collect()
    }
}

impl<S: Send + Sync + Clone> Debug for Contract<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contract")
            .field("name", &self.name)
            .field("addresses", &self.addresses)
            .finish()
    }
}

pub fn get_state_migrations<S: Send + Sync + Clone>(
    contracts: &[Contract<S>],
) -> Vec<Arc<dyn StateMigrations>> {
    contracts.iter().flat_map(|c| c.state_migrations.clone()).collect()
}

pub fn get_pure_handlers<S: Send + Sync + Clone>(
    contracts: &[Contract<S>],
) -> HashMap<EventAbi, Arc<dyn PureHandler>> {
    contracts.iter().fold(HashMap::new(), |mut handlers_by_event_abi, contract| {
        contract.pure_handlers.iter().for_each(|(event_abi, handler)| {
            handlers_by_event_abi.insert(event_abi, handler.clone());
        });
        handlers_by_event_abi
    })
}

pub fn get_side_effect_handlers<S: Send + Sync + Clone>(
    contracts: &[Contract<S>],
) -> HashMap<EventAbi, Arc<dyn SideEffectHandler<SharedState = S>>> {
    contracts.iter().fold(HashMap::new(), |mut handlers_by_event_abi, contract| {
        contract.side_effect_handlers.iter().for_each(|(event_abi, handler)| {
            handlers_by_event_abi.insert(event_abi, handler.clone());
        });
        handlers_by_event_abi
    })
}

pub fn group_event_topics_by_names<S: Send + Sync + Clone>(
    contracts: &[Contract<S>],
) -> HashMap<String, Vec<ContractEventTopic>> {
    contracts.iter().fold(HashMap::new(), |mut topics_by_contract_name, contract| {
        topics_by_contract_name.insert(contract.name.clone(), contract.get_event_topics());

        topics_by_contract_name
    })
}

pub fn group_events_by_topics<S: Send + Sync + Clone>(
    contracts: &[Contract<S>],
) -> HashMap<ContractEventTopic, ContractEvent> {
    contracts
        .iter()
        .flat_map(|c| c.build_events())
        .map(|e| (e.value.signature(), e))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Insertable)]
#[diesel(table_name = chaindexing_contract_addresses)]
pub struct UnsavedContractAddress {
    pub contract_name: String,
    pub address: String,
    pub chain_id: i64,
    pub start_block_number: i64,
    next_block_number_to_ingest_from: i64,
}

impl UnsavedContractAddress {
    pub fn new(
        contract_name: &str,
        address: &str,
        chain_id: &ChainId,
        start_block_number: u64,
    ) -> Self {
        let start_block_number = start_block_number as i64;

        UnsavedContractAddress {
            contract_name: contract_name.to_string(),
            address: address.to_lowercase().to_string(),
            chain_id: *chain_id as i64,
            start_block_number,
            next_block_number_to_ingest_from: start_block_number,
        }
    }
}

// N/B: The order has to match ./schema.rs to stop diesel from mixing up fields
/// Helps manage subscription of ingesting and handling events per contract address
#[derive(Debug, Clone, PartialEq, Queryable, Identifiable, Deserialize)]
#[diesel(table_name = chaindexing_contract_addresses)]
#[diesel(primary_key(id))]
pub struct ContractAddress {
    pub id: i64,
    pub chain_id: i64,
    pub next_block_number_to_ingest_from: i64,
    pub next_block_number_to_handle_from: i64,
    pub next_block_number_for_side_effects: i64,
    pub start_block_number: i64,
    pub address: String,
    pub contract_name: String,
}

impl ContractAddress {
    fn get_chain_id(&self) -> ChainId {
        U64::from(self.chain_id).try_into().unwrap()
    }

    pub fn group_contract_addresses_by_address_and_chain_id(
        contract_addresses: &[ContractAddress],
    ) -> HashMap<(Address, ChainId), &ContractAddress> {
        contract_addresses.iter().fold(
            HashMap::new(),
            |mut contracts_by_addresses, contract_address @ ContractAddress { address, .. }| {
                contracts_by_addresses.insert(
                    (
                        Address::from_str(address.as_str()).unwrap(),
                        contract_address.get_chain_id(),
                    ),
                    contract_address,
                );

                contracts_by_addresses
            },
        )
    }
}
