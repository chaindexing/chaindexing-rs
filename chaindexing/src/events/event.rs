use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::contracts::UnsavedContractAddress;
use crate::diesels::schema::chaindexing_events;
use crate::utils::address_to_string;
use diesel::{Insertable, Queryable};
use ethers::abi::{LogParam, Token};
use ethers::types::{Address, Log, U256};

use crate::{hashes, ContractEvent};
use uuid::Uuid;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Eq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_events)]
pub struct Event {
    pub id: Uuid,
    pub chain_id: i64,
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
    pub transaction_index: i32,
    pub log_index: i32,
    removed: bool,
    inserted_at: chrono::NaiveDateTime,
}

/// Introduced to allow computing with a subset of Event struct
#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
pub struct PartialEvent {
    pub id: Uuid,
    pub chain_id: i64,
    pub contract_address: String,
    pub contract_name: String,
    pub block_hash: String,
    pub block_number: i64,
    pub block_timestamp: i64,
    pub transaction_hash: String,
    pub transaction_index: i64,
    pub log_index: i64,
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
            contract_address: address_to_string(&log.address).to_lowercase(),
            contract_name: contract_address.contract_name.to_owned(),
            abi: event.abi.clone(),
            log_params: serde_json::to_value(log_params).unwrap(),
            parameters: serde_json::to_value(parameters).unwrap(),
            topics: serde_json::to_value(&log.topics).unwrap(),
            block_hash: hashes::h256_to_string(&log.block_hash.unwrap()).to_lowercase(),
            block_number: log.block_number.unwrap().as_u64() as i64,
            block_timestamp,
            transaction_hash: hashes::h256_to_string(&log.transaction_hash.unwrap()).to_lowercase(),
            transaction_index: log.transaction_index.unwrap().as_u32() as i32,
            log_index: log.log_index.unwrap().as_u32() as i32,
            removed: log.removed.unwrap(),
            inserted_at: chrono::Utc::now().naive_utc(),
        }
    }

    pub fn get_params(&self) -> EventParam {
        EventParam::new(&self.parameters)
    }

    pub fn not_removed(&self) -> bool {
        !self.removed
    }

    pub fn match_contract_address(&self, contract_address: &str) -> bool {
        self.contract_address.to_lowercase() == *contract_address.to_lowercase()
    }

    fn log_params_to_parameters(log_params: &[LogParam]) -> HashMap<String, Token> {
        log_params.iter().fold(HashMap::new(), |mut parameters, log_param| {
            parameters.insert(log_param.name.to_string(), log_param.value.clone());

            parameters
        })
    }
}

pub struct EventParam {
    value: HashMap<String, Token>,
}

impl EventParam {
    pub fn new(parameters: &serde_json::Value) -> EventParam {
        EventParam {
            value: serde_json::from_value(parameters.clone()).unwrap(),
        }
    }

    pub fn get_string(&self, key: &str) -> String {
        self.value.get(key).unwrap().to_string()
    }
    pub fn get_u8_string(&self, key: &str) -> String {
        self.get_u8(key).to_string()
    }
    pub fn get_u8(&self, key: &str) -> u8 {
        self.get_usize(key) as u8
    }
    pub fn get_usize(&self, key: &str) -> usize {
        self.get_uint(key).as_usize()
    }
    pub fn get_u32_string(&self, key: &str) -> String {
        self.get_u32(key).to_string()
    }
    pub fn get_u32(&self, key: &str) -> u32 {
        self.get_uint(key).as_u32()
    }
    pub fn get_u64_string(&self, key: &str) -> String {
        self.get_u64(key).to_string()
    }
    pub fn get_u64(&self, key: &str) -> u64 {
        self.get_uint(key).as_u64()
    }
    pub fn get_u128_string(&self, key: &str) -> String {
        self.get_u128(key).to_string()
    }
    pub fn get_u128(&self, key: &str) -> u128 {
        self.get_uint(key).as_u128()
    }
    /// Same as _u256
    pub fn get_uint_string(&self, key: &str) -> String {
        self.get_uint(key).to_string()
    }
    pub fn get_uint(&self, key: &str) -> U256 {
        self.value.get(key).unwrap().clone().into_uint().unwrap()
    }
    pub fn get_address_string(&self, key: &str) -> String {
        address_to_string(&self.get_address(key)).to_lowercase()
    }
    pub fn get_address(&self, key: &str) -> Address {
        self.value.get(key).unwrap().clone().into_address().unwrap()
    }
}
