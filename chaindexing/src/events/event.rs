use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use crate::diesel::schema::chaindexing_events;
use alloy::dyn_abi::{DecodedEvent, DynSolValue, EventExt};
use alloy::primitives::{utils::format_ether, Address, I256, U256};
use alloy::rpc::types::Log;
use diesel::{Insertable, Queryable};
use serde_json::{json, Value};

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
    status: String,
    reorg_id: Option<i64>,
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
            && self.transaction_hash == other.transaction_hash
            && self.log_index == other.log_index
    }
}

impl Hash for Event {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.chain_id.hash(state);
        self.contract_address.hash(state);
        self.abi.hash(state);
        self.block_hash.hash(state);
        self.transaction_hash.hash(state);
        self.log_index.hash(state);
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
        let decoded_log = event.value.decode_log(log.data()).unwrap();
        let parameters = Self::decoded_log_to_parameters(event, decoded_log);

        Self {
            id: uuid::Uuid::new_v4(),
            chain_id: *chain_id as i64,
            contract_address: utils::address_to_string(&log.address()).to_lowercase(),
            contract_name: contract_name.to_owned(),
            abi: event.abi.clone(),
            parameters: parameters_to_json(&parameters),
            topics: serde_json::to_value(log.topics()).unwrap(),
            block_hash: hashes::h256_to_string(&log.block_hash.unwrap()).to_lowercase(),
            block_number: log.block_number.unwrap() as i64,
            block_timestamp,
            transaction_hash: hashes::h256_to_string(&log.transaction_hash.unwrap()).to_lowercase(),
            transaction_index: log.transaction_index.unwrap() as i32,
            log_index: log.log_index.unwrap() as i32,
            removed: log.removed,
            status: "canonical".to_string(),
            reorg_id: None,
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
        ChainId::try_from(self.chain_id as u64).unwrap()
    }

    fn decoded_log_to_parameters(
        event: &ContractEvent,
        decoded_log: DecodedEvent,
    ) -> HashMap<String, EventParamValue> {
        let mut indexed = decoded_log.indexed.into_iter();
        let mut body = decoded_log.body.into_iter();

        event.value.inputs.iter().fold(HashMap::new(), |mut parameters, input| {
            let value = if input.indexed {
                indexed.next()
            } else {
                body.next()
            }
            .unwrap();

            parameters.insert(input.name.to_string(), EventParamValue::from(value));
            parameters
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum EventParamValue {
    Address(Address),
    Uint(U256),
    Int(I256),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    FixedBytes(Vec<u8>),
    Array(Vec<EventParamValue>),
    FixedArray(Vec<EventParamValue>),
    Tuple(Vec<EventParamValue>),
}

impl EventParamValue {
    fn to_json(&self) -> Value {
        match self {
            EventParamValue::Address(value) => {
                json!({ "Address": utils::address_to_string(value) })
            }
            EventParamValue::Uint(value) => json!({ "Uint": u256_to_hex(*value) }),
            EventParamValue::Int(value) => json!({ "Int": u256_to_hex(value.into_raw()) }),
            EventParamValue::Bool(value) => json!({ "Bool": value }),
            EventParamValue::String(value) => json!({ "String": value }),
            EventParamValue::Bytes(value) => json!({ "Bytes": value }),
            EventParamValue::FixedBytes(value) => json!({ "FixedBytes": value }),
            EventParamValue::Array(values) => {
                json!({ "Array": values.iter().map(EventParamValue::to_json).collect::<Vec<_>>() })
            }
            EventParamValue::FixedArray(values) => {
                json!({ "FixedArray": values.iter().map(EventParamValue::to_json).collect::<Vec<_>>() })
            }
            EventParamValue::Tuple(values) => {
                json!({ "Tuple": values.iter().map(EventParamValue::to_json).collect::<Vec<_>>() })
            }
        }
    }

    fn from_json(value: &Value) -> Self {
        let object = value.as_object().expect("event parameter value must be an object");
        let (kind, value) = object.iter().next().expect("event parameter value must be tagged");

        match kind.as_str() {
            "Address" => {
                EventParamValue::Address(Address::from_str(value.as_str().unwrap()).unwrap())
            }
            "Uint" => EventParamValue::Uint(parse_u256(value)),
            "Int" => EventParamValue::Int(parse_i256(value)),
            "Bool" => EventParamValue::Bool(value.as_bool().unwrap()),
            "String" => EventParamValue::String(value.as_str().unwrap().to_string()),
            "Bytes" => EventParamValue::Bytes(parse_bytes(value)),
            "FixedBytes" => EventParamValue::FixedBytes(parse_bytes(value)),
            "Array" => EventParamValue::Array(parse_values(value)),
            "FixedArray" => EventParamValue::FixedArray(parse_values(value)),
            "Tuple" => EventParamValue::Tuple(parse_values(value)),
            _ => panic!("unsupported event parameter type: {kind}"),
        }
    }

    fn into_uint(self) -> U256 {
        match self {
            EventParamValue::Uint(value) => value,
            _ => panic!("event parameter is not a uint"),
        }
    }

    fn into_int(self) -> I256 {
        match self {
            EventParamValue::Int(value) => value,
            _ => panic!("event parameter is not an int"),
        }
    }

    fn into_address(self) -> Address {
        match self {
            EventParamValue::Address(value) => value,
            _ => panic!("event parameter is not an address"),
        }
    }
}

impl From<DynSolValue> for EventParamValue {
    fn from(value: DynSolValue) -> Self {
        match value {
            DynSolValue::Bool(value) => EventParamValue::Bool(value),
            DynSolValue::Int(value, _) => EventParamValue::Int(value),
            DynSolValue::Uint(value, _) => EventParamValue::Uint(value),
            DynSolValue::FixedBytes(value, size) => {
                EventParamValue::FixedBytes(value[..size.min(32)].to_vec())
            }
            DynSolValue::Address(value) => EventParamValue::Address(value),
            DynSolValue::Function(value) => {
                EventParamValue::FixedBytes(value.into_word()[..24].to_vec())
            }
            DynSolValue::Bytes(value) => EventParamValue::Bytes(value),
            DynSolValue::String(value) => EventParamValue::String(value),
            DynSolValue::Array(values) => {
                EventParamValue::Array(values.into_iter().map(EventParamValue::from).collect())
            }
            DynSolValue::FixedArray(values) => {
                EventParamValue::FixedArray(values.into_iter().map(EventParamValue::from).collect())
            }
            DynSolValue::Tuple(values) => {
                EventParamValue::Tuple(values.into_iter().map(EventParamValue::from).collect())
            }
        }
    }
}

impl std::fmt::Display for EventParamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventParamValue::Address(value) => {
                let address = utils::address_to_string(value);
                write!(f, "{}", address.strip_prefix("0x").unwrap_or(&address))
            }
            EventParamValue::Uint(value) => write!(f, "{value:x}"),
            EventParamValue::Int(value) => write!(f, "{:x}", value.into_raw()),
            EventParamValue::Bool(value) => write!(f, "{value}"),
            EventParamValue::String(value) => value.fmt(f),
            EventParamValue::Bytes(value) | EventParamValue::FixedBytes(value) => {
                write!(f, "{}", alloy::hex::encode(value))
            }
            EventParamValue::Array(values) | EventParamValue::FixedArray(values) => write!(
                f,
                "[{}]",
                values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
            ),
            EventParamValue::Tuple(values) => write!(
                f,
                "({})",
                values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
            ),
        }
    }
}

fn parameters_to_json(parameters: &HashMap<String, EventParamValue>) -> Value {
    let mut object = serde_json::Map::new();
    for (key, value) in parameters {
        object.insert(key.clone(), value.to_json());
    }
    Value::Object(object)
}

fn parse_values(value: &Value) -> Vec<EventParamValue> {
    value.as_array().unwrap().iter().map(EventParamValue::from_json).collect()
}

fn parse_bytes(value: &Value) -> Vec<u8> {
    if let Some(hex) = value.as_str() {
        return alloy::hex::decode(hex.strip_prefix("0x").unwrap_or(hex)).unwrap();
    }

    value
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| byte.as_u64().unwrap() as u8)
        .collect()
}

fn parse_u256(value: &Value) -> U256 {
    if let Some(number) = value.as_u64() {
        return U256::from(number);
    }

    let value = value.as_str().unwrap();
    if let Some(hex) = value.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).unwrap()
    } else {
        U256::from_str_radix(value, 10).unwrap()
    }
}

fn parse_i256(value: &Value) -> I256 {
    if let Some(number) = value.as_i64() {
        return I256::try_from(number).unwrap();
    }

    let value = value.as_str().unwrap();
    if let Some(hex) = value.strip_prefix("0x") {
        I256::from_raw(U256::from_str_radix(hex, 16).unwrap())
    } else if value.starts_with('-') {
        I256::from_dec_str(value).unwrap()
    } else {
        I256::from_raw(U256::from_str_radix(value, 10).unwrap())
    }
}

fn u256_to_hex(value: U256) -> String {
    format!("{value:#x}")
}

/// Represents the parameters parsed from an event/log.
/// Contains convenient parsers to convert or transform into useful primitives
/// as needed.
pub struct EventParam {
    value: HashMap<String, EventParamValue>,
}

impl EventParam {
    pub(crate) fn new(parameters: &serde_json::Value) -> EventParam {
        let value = parameters
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), EventParamValue::from_json(value)))
            .collect();

        EventParam { value }
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

        match token {
            EventParamValue::Bytes(value) | EventParamValue::FixedBytes(value) => value,
            _ => panic!("event parameter is not bytes"),
        }
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
        self.get_array_and_transform(key, |token| i128::try_from(token_to_int(token)).unwrap())
    }

    pub fn get_u8_array(&self, key: &str) -> Vec<u8> {
        self.get_array_and_transform(key, |token| token_to_uint(token).to::<usize>() as u8)
    }
    pub fn get_u32_array(&self, key: &str) -> Vec<u32> {
        self.get_array_and_transform(key, |token| token_to_uint(token).to::<u32>())
    }
    pub fn get_u64_array(&self, key: &str) -> Vec<u64> {
        self.get_array_and_transform(key, |token| token_to_uint(token).to::<u64>())
    }
    pub fn get_u128_array(&self, key: &str) -> Vec<u128> {
        self.get_array_and_transform(key, |token| token_to_uint(token).to::<u128>())
    }
    pub fn get_uint_array(&self, key: &str) -> Vec<U256> {
        self.get_array_and_transform(key, token_to_uint)
    }
    pub fn get_int_array(&self, key: &str) -> Vec<I256> {
        self.get_array_and_transform(key, token_to_int)
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
        TokenTransformer: Fn(EventParamValue) -> Output,
    {
        self.get_array(key).into_iter().map(token_transformer).collect()
    }
    fn get_array(&self, key: &str) -> Vec<EventParamValue> {
        let token = self.get_token(key);

        match token {
            EventParamValue::Array(values) | EventParamValue::FixedArray(values) => values,
            _ => panic!("event parameter is not an array"),
        }
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
        i128::try_from(self.get_int(key)).unwrap()
    }

    pub fn get_u8(&self, key: &str) -> u8 {
        self.get_usize(key) as u8
    }
    pub fn get_usize(&self, key: &str) -> usize {
        self.get_uint(key).to::<usize>()
    }
    pub fn get_u32(&self, key: &str) -> u32 {
        self.get_uint(key).to::<u32>()
    }
    pub fn get_u64(&self, key: &str) -> u64 {
        self.get_uint(key).to::<u64>()
    }
    pub fn get_u128(&self, key: &str) -> u128 {
        self.get_uint(key).to::<u128>()
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

    fn get_token(&self, key: &str) -> EventParamValue {
        self.value.get(key).unwrap().clone()
    }
}

fn token_to_address_string(token: EventParamValue) -> String {
    utils::address_to_string(&token_to_address(token)).to_lowercase()
}

fn token_to_address(token: EventParamValue) -> Address {
    token.into_address()
}

fn token_to_uint(token: EventParamValue) -> U256 {
    token.into_uint()
}
fn token_to_int(token: EventParamValue) -> I256 {
    token.into_int()
}

const GWEI: f64 = 1_000_000_000.0;

mod hashes {
    use alloy::primitives::B256;

    pub fn h256_to_string(h256: &B256) -> String {
        h256.to_string()
    }
}

mod utils {
    use alloy::primitives::Address;

    pub fn address_to_string(address: &Address) -> String {
        format!("{address:?}")
    }
}

#[cfg(test)]
mod event_param_tests {
    use std::collections::HashSet;

    use alloy::primitives::{Bytes, Log as PrimitiveLog, LogData, B256};
    use serde_json::json;

    use super::*;

    #[test]
    fn returns_uint_values() {
        let event_param =
            EventParam::new(&json!({"sqrtPriceX96":{"Uint":"0x1ca2dce57b617d43d62181e8"}}));
        assert_eq!(
            event_param.get_uint("sqrtPriceX96"),
            U256::from_str_radix("8862469411596380921745474024", 10).unwrap()
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

    #[test]
    fn address_params_serialize_as_lowercase_raw_hex() {
        let contract_event = ContractEvent::new("event OwnerChanged(address indexed owner)");
        let owner = Address::from_str("0xd8da6bf26964af9d7eed9e03e53415d37aa96045").unwrap();
        let log = Log {
            inner: PrimitiveLog {
                address: Address::from_str("0x0000000000000000000000000000000000000003").unwrap(),
                data: LogData::new_unchecked(
                    vec![contract_event.value.selector(), owner.into_word()],
                    Bytes::new(),
                ),
            },
            block_hash: Some(B256::from(U256::from(10).to_be_bytes::<32>())),
            block_number: Some(10),
            transaction_hash: Some(B256::from(U256::from(20).to_be_bytes::<32>())),
            transaction_index: Some(1),
            log_index: Some(2),
            removed: false,
            ..Default::default()
        };

        let event = Event::new(&log, &contract_event, &ChainId::Mainnet, "Registry", 123);
        let params = event.get_params();

        assert_eq!(
            event.parameters["owner"]["Address"],
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
        );
        assert_eq!(
            params.get_address_string("owner"),
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
        );
        assert_eq!(
            params.get_string_unsafely("owner"),
            "d8da6bf26964af9d7eed9e03e53415d37aa96045"
        );
    }

    #[test]
    fn string_unsafely_keeps_legacy_non_string_token_formatting() {
        let event_param = EventParam::new(&json!({
            "amount": {"Uint": "0xff"},
            "raw_amount": {"Int": "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0"},
            "data": {"FixedBytes": [222, 173, 190, 239]},
            "values": {"Array": [{"Uint": "0x1"}, {"Uint": "0x2"}]},
            "tuple": {"Tuple": [{"Bool": true}, {"String": "ok"}]}
        }));

        assert_eq!(event_param.get_string_unsafely("amount"), "ff");
        assert_eq!(
            event_param.get_string_unsafely("raw_amount"),
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0"
        );
        assert_eq!(event_param.get_string_unsafely("data"), "deadbeef");
        assert_eq!(event_param.get_string_unsafely("values"), "[1,2]");
        assert_eq!(event_param.get_string_unsafely("tuple"), "(true,ok)");
    }

    #[test]
    fn event_new_decodes_alloy_logs_into_event_params() {
        let contract_event = ContractEvent::new(
            "event Transfer(address indexed from, address indexed to, uint256 value)",
        );
        let from = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let to = Address::from_str("0x0000000000000000000000000000000000000002").unwrap();
        let value = B256::from(U256::from(100).to_be_bytes::<32>());
        let log = Log {
            inner: PrimitiveLog {
                address: Address::from_str("0x0000000000000000000000000000000000000003").unwrap(),
                data: LogData::new_unchecked(
                    vec![
                        contract_event.value.selector(),
                        from.into_word(),
                        to.into_word(),
                    ],
                    Bytes::copy_from_slice(value.as_slice()),
                ),
            },
            block_hash: Some(B256::from(U256::from(10).to_be_bytes::<32>())),
            block_number: Some(10),
            transaction_hash: Some(B256::from(U256::from(20).to_be_bytes::<32>())),
            transaction_index: Some(1),
            log_index: Some(2),
            removed: false,
            ..Default::default()
        };

        let event = Event::new(&log, &contract_event, &ChainId::Mainnet, "ERC20", 123);
        let params = event.get_params();

        assert_eq!(params.get_address("from"), from);
        assert_eq!(params.get_address("to"), to);
        assert_eq!(params.get_uint("value"), U256::from(100));
    }

    fn event_with_identity(transaction_hash: &str, log_index: i32) -> Event {
        Event {
            id: uuid::Uuid::new_v4(),
            chain_id: 1,
            contract_address: "0x0000000000000000000000000000000000000001".to_string(),
            contract_name: "ERC721".to_string(),
            abi:
                "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
                    .to_string(),
            parameters: serde_json::Value::Null,
            topics: serde_json::Value::Null,
            block_hash: "0xblock".to_string(),
            block_number: 10,
            block_timestamp: 100,
            transaction_hash: transaction_hash.to_string(),
            transaction_index: 1,
            log_index,
            removed: false,
            status: "canonical".to_string(),
            reorg_id: None,
        }
    }

    #[test]
    fn event_identity_distinguishes_events_in_same_block() {
        let first_event = event_with_identity("0xtx1", 0);
        let second_event = event_with_identity("0xtx2", 1);

        assert_ne!(first_event, second_event);

        let mut events = HashSet::new();
        events.insert(first_event);
        events.insert(second_event);

        assert_eq!(events.len(), 2);
    }
}
