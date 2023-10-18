use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::contracts::{ContractAddress, Contracts, UnsavedContractAddress};
use crate::diesels::schema::chaindexing_events;
use crate::hashes::Hashes;
use diesel::{Insertable, Queryable};
use ethers::abi::{LogParam, Token};
use ethers::types::{Block, Log, TxHash};

use crate::{Contract, ContractEvent};
use uuid::Uuid;

#[derive(Debug, Clone, Eq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_events)]
pub struct Event {
    pub id: Uuid,
    pub chain_id: i32,
    pub contract_address: String,
    pub contract_name: String,
    pub abi: String,
    log_params: serde_json::Value,
    parameters: serde_json::Value,
    topics: serde_json::Value,
    pub block_hash: String,
    pub block_number: i64,
    pub block_timestamp: i64,
    pub transaction_hash: String,
    pub transaction_index: i64,
    pub log_index: i64,
    removed: bool,
    inserted_at: chrono::NaiveDateTime,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.chain_id == other.chain_id
            && self.contract_address == other.contract_address
            && self.abi == other.abi
            && self.log_params == other.log_params
            && self.block_hash == other.block_hash
    }
}

impl Hash for Event {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.chain_id.hash(state);
        self.contract_address.hash(state);
        self.abi.hash(state);
        self.log_params.to_string().hash(state);
        self.block_hash.hash(state);
    }
}

impl Event {
    pub fn new(
        log: &Log,
        event: &ContractEvent,
        contract_address: &UnsavedContractAddress,
        block_timestamp: i64,
    ) -> Self {
        let log_params = event.value.parse_log(log.clone().into()).unwrap().params;
        let parameters = Self::log_params_to_parameters(&log_params);

        Self {
            id: uuid::Uuid::new_v4(),
            chain_id: contract_address.chain_id,
            contract_address: ContractAddress::address_to_string(&log.address).to_lowercase(),
            contract_name: contract_address.contract_name.to_owned(),
            abi: event.abi.clone(),
            log_params: serde_json::to_value(log_params).unwrap(),
            parameters: serde_json::to_value(parameters).unwrap(),
            topics: serde_json::to_value(&log.topics).unwrap(),
            block_hash: Hashes::h256_to_string(&log.block_hash.unwrap()).to_lowercase(),
            block_number: log.block_number.unwrap().as_u64() as i64,
            block_timestamp,
            transaction_hash: Hashes::h256_to_string(&log.transaction_hash.unwrap()).to_lowercase(),
            transaction_index: log.transaction_index.unwrap().as_u64() as i64,
            log_index: log.log_index.unwrap().as_u64() as i64,
            removed: log.removed.unwrap(),
            inserted_at: chrono::Utc::now().naive_utc(),
        }
    }

    pub fn get_params(&self) -> HashMap<String, Token> {
        serde_json::from_value(self.parameters.clone()).unwrap()
    }

    pub fn not_removed(&self) -> bool {
        !self.removed
    }

    pub fn match_contract_address(&self, contract_address: &String) -> bool {
        self.contract_address.to_lowercase() == *contract_address.to_lowercase()
    }

    fn log_params_to_parameters(log_params: &Vec<LogParam>) -> HashMap<String, Token> {
        log_params.iter().fold(HashMap::new(), |mut parameters, log_param| {
            parameters.insert(log_param.name.to_string(), log_param.value.clone());

            parameters
        })
    }
}

pub struct Events;

impl Events {
    pub fn new(
        logs: &Vec<Log>,
        contracts: &Vec<Contract>,
        blocks_by_tx_hash: &HashMap<TxHash, Block<TxHash>>,
    ) -> Vec<Event> {
        let events_by_topics = Contracts::group_events_by_topics(contracts);
        let contract_addresses_by_address =
            Contracts::get_all_contract_addresses_grouped_by_address(contracts);

        logs.iter()
            .map(
                |log @ Log {
                     topics,
                     address,
                     transaction_hash,
                     ..
                 }| {
                    let contract_address = contract_addresses_by_address.get(&address).unwrap();
                    let block = blocks_by_tx_hash.get(&transaction_hash.unwrap()).unwrap();

                    Event::new(
                        log,
                        &events_by_topics.get(&topics[0]).unwrap(),
                        &contract_address,
                        block.timestamp.as_u64() as i64,
                    )
                },
            )
            .collect()
    }
}
