use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::diesel::schema::chaindexing_events;
use diesel::{Insertable, Queryable};
use ethers::abi::{LogParam, Token};
use ethers::types::{Address, Log, I256, U256, U64};
use ethers::utils::format_ether;

use crate::{ChainId, ContractEvent};
use uuid::Uuid;

use serde::Deserialize;

/// Events, aka. provider logs, are emitted from smart contracts
/// to help infer their states.
#[derive(Debug, Deserialize, Clone, Eq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_events)]
pub struct Event {
    pub id: Uuid,
    pub(crate) chain_id: i64,
    pub contract_address: String,
    pub contract_name: String,
    pub abi: String,
    parameters: serde_json::Value,
    topics: serde_json::Value,
    pub block_hash: String,
    pub(crate) block_number: i64,
    block_timestamp: i64,
    pub transaction_hash: String,
    pub(crate) transaction_index: i32,
    pub(crate) log_index: i32,
    removed: bool,
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
            && self.block_hash == other.block_hash
    }
}

impl Hash for Event {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.chain_id.hash(state);
        self.contract_address.hash(state);
        self.abi.hash(state);
        self.block_hash.hash(state);
    }
}

impl Event {
    pub fn new(
        log: &Log,
        event: &ContractEvent,
        chain_id: &ChainId,
        contract_name: &str,
        block_timestamp: i64,
    ) -> Self {
        let log_params = event.value.parse_log(log.clone().into()).unwrap().params;
        let parameters = Self::log_params_to_parameters(&log_params);

        Self {
            id: uuid::Uuid::new_v4(),
            chain_id: *chain_id as i64,
            contract_address: utils::address_to_string(&log.address).to_lowercase(),
            contract_name: contract_name.to_owned(),
            abi: event.abi.clone(),
            parameters: serde_json::to_value(parameters).unwrap(),
            topics: serde_json::to_value(&log.topics).unwrap(),
            block_hash: hashes::h256_to_string(&log.block_hash.unwrap()).to_lowercase(),
            block_number: log.block_number.unwrap().as_u64() as i64,
            block_timestamp,
            transaction_hash: hashes::h256_to_string(&log.transaction_hash.unwrap()).to_lowercase(),
            transaction_index: log.transaction_index.unwrap().as_u32() as i32,
            log_index: log.log_index.unwrap().as_u32() as i32,
            removed: log.removed.unwrap(),
        }
    }

    pub(crate) fn get_abi(&self) -> &str {
        self.abi.as_str()
    }

    /// Returns the event's block number
    pub fn get_block_number(&self) -> u64 {
        self.block_number as u64
    }
    /// Returns the event's block timestamp
    pub fn get_block_timestamp(&self) -> u64 {
        self.block_timestamp as u64
    }
    /// Returns the event's transaction index
    pub fn get_transaction_index(&self) -> u32 {
        self.transaction_index as u32
    }
    /// Returns the event's log index
    pub fn get_log_index(&self) -> u32 {
        self.log_index as u32
    }

    /// Returns the event's parameters
    pub fn get_params(&self) -> EventParam {
        EventParam::new(&self.parameters)
    }

    /// Returns the event's chain id
    pub fn get_chain_id(&self) -> ChainId {
        U64::from(self.chain_id).try_into().unwrap()
    }

    fn log_params_to_parameters(log_params: &[LogParam]) -> HashMap<String, Token> {
        log_params.iter().fold(HashMap::new(), |mut parameters, log_param| {
            parameters.insert(log_param.name.to_string(), log_param.value.clone());

            parameters
        })
    }
}

/// Represents the parameters parsed from an event/log.
/// Contains convenient parsers to convert or transform into useful primitives
/// as needed.
pub struct EventParam {
    value: HashMap<String, Token>,
}

impl EventParam {
    pub(crate) fn new(parameters: &serde_json::Value) -> EventParam {
        EventParam {
            value: serde_json::from_value(parameters.clone()).unwrap(),
        }
    }

    /// N/B: This function is UNSAFE.
    /// Ensure source contract can be trusted before using it or
    /// preprocess the string before indexing.
    /// A potential attacker could inject SQL string statements from  here.
    pub fn get_string_unsafely(&self, key: &str) -> String {
        self.value.get(key).unwrap().to_string()
    }

    /// Returns `bytes` or bytes1, bytes2. bytes3...bytes32
    pub fn get_bytes(&self, key: &str) -> Vec<u8> {
        let token = self.get_token(key);

        token.clone().into_fixed_bytes().or(token.into_bytes()).unwrap()
    }

    pub fn get_i8_array(&self, key: &str) -> Vec<i8> {
        self.get_array_and_transform(key, |token| token_to_int(token).as_i8())
    }
    pub fn get_i32_array(&self, key: &str) -> Vec<i32> {
        self.get_array_and_transform(key, |token| token_to_int(token).as_i32())
    }
    pub fn get_i64_array(&self, key: &str) -> Vec<i64> {
        self.get_array_and_transform(key, |token| token_to_int(token).as_i64())
    }
    pub fn get_i128_array(&self, key: &str) -> Vec<i128> {
        self.get_array_and_transform(key, |token| token_to_int(token).as_i128())
    }

    pub fn get_u8_array(&self, key: &str) -> Vec<u8> {
        self.get_array_and_transform(key, |token| token_to_uint(token).as_usize() as u8)
    }
    pub fn get_u32_array(&self, key: &str) -> Vec<u32> {
        self.get_array_and_transform(key, |token| token_to_uint(token).as_u32())
    }
    pub fn get_u64_array(&self, key: &str) -> Vec<u64> {
        self.get_array_and_transform(key, |token| token_to_uint(token).as_u64())
    }
    pub fn get_u128_array(&self, key: &str) -> Vec<u128> {
        self.get_array_and_transform(key, |token| token_to_uint(token).as_u128())
    }
    pub fn get_uint_array(&self, key: &str) -> Vec<U256> {
        self.get_array_and_transform(key, token_to_uint)
    }
    pub fn get_int_array(&self, key: &str) -> Vec<I256> {
        self.get_array_and_transform(key, |token| I256::from_raw(token.into_int().unwrap()))
    }

    pub fn get_address_array(&self, key: &str) -> Vec<Address> {
        self.get_array_and_transform(key, token_to_address)
    }
    pub fn get_address_string_array(&self, key: &str) -> Vec<String> {
        self.get_array_and_transform(key, |token| token_to_address_string(token).to_lowercase())
    }

    fn get_array_and_transform<TokenTransformer, Output>(
        &self,
        key: &str,
        token_transformer: TokenTransformer,
    ) -> Vec<Output>
    where
        TokenTransformer: Fn(Token) -> Output,
    {
        self.get_array(key).into_iter().map(token_transformer).collect()
    }
    fn get_array(&self, key: &str) -> Vec<Token> {
        let token = self.get_token(key);

        token.clone().into_fixed_array().or(token.into_array()).unwrap()
    }

    pub fn get_int_gwei(&self, key: &str) -> f64 {
        self.get_int_ether(key) * GWEI
    }
    pub fn get_int_ether(&self, key: &str) -> f64 {
        format_ether(self.get_int(key)).parse().unwrap()
    }

    pub fn get_uint_gwei(&self, key: &str) -> f64 {
        self.get_uint_ether(key) * GWEI
    }
    pub fn get_uint_ether(&self, key: &str) -> f64 {
        format_ether(self.get_uint(key)).parse().unwrap()
    }

    pub fn get_i8(&self, key: &str) -> i8 {
        self.get_int(key).as_i8()
    }
    pub fn get_i32(&self, key: &str) -> i32 {
        self.get_int(key).as_i32()
    }
    pub fn get_i64(&self, key: &str) -> i64 {
        self.get_int(key).as_i64()
    }
    pub fn get_i128(&self, key: &str) -> i128 {
        self.get_int(key).as_i128()
    }

    pub fn get_u8(&self, key: &str) -> u8 {
        self.get_usize(key) as u8
    }
    pub fn get_usize(&self, key: &str) -> usize {
        self.get_uint(key).as_usize()
    }
    pub fn get_u32(&self, key: &str) -> u32 {
        self.get_uint(key).as_u32()
    }
    pub fn get_u64(&self, key: &str) -> u64 {
        self.get_uint(key).as_u64()
    }
    pub fn get_u128(&self, key: &str) -> u128 {
        self.get_uint(key).as_u128()
    }
    /// Same as get_u256
    pub fn get_uint(&self, key: &str) -> U256 {
        token_to_uint(self.get_token(key))
    }
    pub fn get_int(&self, key: &str) -> I256 {
        token_to_int(self.get_token(key))
    }
    pub fn get_address_string(&self, key: &str) -> String {
        token_to_address_string(self.get_token(key))
    }
    pub fn get_address(&self, key: &str) -> Address {
        token_to_address(self.get_token(key))
    }

    fn get_token(&self, key: &str) -> Token {
        self.value.get(key).unwrap().clone()
    }
}

fn token_to_address_string(token: Token) -> String {
    utils::address_to_string(&token_to_address(token)).to_lowercase()
}

fn token_to_address(token: Token) -> Address {
    token.into_address().unwrap()
}

fn token_to_uint(token: Token) -> U256 {
    token.into_uint().unwrap()
}
fn token_to_int(token: Token) -> I256 {
    I256::from_raw(token.into_int().unwrap())
}

const GWEI: f64 = 1_000_000_000.0;

mod hashes {
    use ethers::types::{H160, H256};

    pub fn h160_to_string(h160: &H160) -> String {
        serde_json::to_value(h160).unwrap().as_str().unwrap().to_string()
    }

    pub fn h256_to_string(h256: &H256) -> String {
        serde_json::to_value(h256).unwrap().as_str().unwrap().to_string()
    }
}

mod utils {
    use ethers::types::H160;

    use super::hashes;

    pub fn address_to_string(address: &H160) -> String {
        hashes::h160_to_string(address)
    }
}

#[cfg(test)]
mod event_param_tests {
    use ethers::types::I256;
    use serde_json::json;

    use super::*;

    #[test]
    fn returns_uint_values() {
        let event_param =
            EventParam::new(&json!({"sqrtPriceX96":{"Uint":"0x1ca2dce57b617d43d62181e8"}}));
        assert_eq!(
            event_param.get_uint("sqrtPriceX96"),
            U256::from_dec_str("8862469411596380921745474024").unwrap()
        );
    }

    #[test]
    fn returns_int_values() {
        let event_param = EventParam::new(
            &json!({"amount0":{"Int":"0xfffffffffffffffffffffffffffffffffffffffffffffffe92da20f7358d10e9"}}),
        );
        assert_eq!(
            event_param.get_int("amount0"),
            I256::from_dec_str("-26311681626831253271").unwrap()
        );
    }
}
