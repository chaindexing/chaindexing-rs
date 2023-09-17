use std::collections::HashMap;

use crate::{contracts::Contracts, diesel::schema::chaindexing_events, ContractAddress};
use diesel::{Insertable, Queryable};
use ethers::{
    abi::{LogParam, Token},
    types::Log,
};

use crate::{Contract, ContractEvent};
use uuid::Uuid;

/// Ingested EVM Events
#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_events)]
pub struct Event {
    pub id: Uuid,
    pub contract_address: String,
    contract_name: String,
    abi: String,
    log_params: serde_json::Value,
    parameters: serde_json::Value,
    topics: serde_json::Value,
    block_hash: String,
    block_number: i64,
    pub transaction_hash: String,
    pub transaction_index: i64,
    pub log_index: i64,
    removed: bool,
    inserted_at: chrono::NaiveDateTime,
}

impl Event {
    pub fn new(log: &Log, event: &ContractEvent) -> Self {
        let log_params = event.value.parse_log(log.clone().into()).unwrap().params;
        let parameters = Self::log_params_to_parameters(&log_params);

        Self {
            id: uuid::Uuid::new_v4(),
            contract_address: ContractAddress::address_to_string(&log.address),
            contract_name: event.contract_name.to_owned(),
            abi: event.abi.clone(),
            log_params: serde_json::to_value(log_params).unwrap(),
            parameters: serde_json::to_value(parameters).unwrap(),
            topics: serde_json::to_value(&log.topics).unwrap(),
            block_hash: log.block_hash.unwrap().to_string(),
            block_number: log.block_number.unwrap().as_u64() as i64,
            transaction_hash: log.transaction_hash.unwrap().to_string(),
            transaction_index: log.transaction_index.unwrap().as_u64() as i64,
            log_index: log.log_index.unwrap().as_u64() as i64,
            removed: log.removed.unwrap(),
            inserted_at: chrono::Utc::now().naive_utc(),
        }
    }

    fn log_params_to_parameters(log_params: &Vec<LogParam>) -> HashMap<String, Token> {
        log_params
            .iter()
            .fold(HashMap::new(), |mut parameters, log_param| {
                parameters.insert(log_param.name.to_string(), log_param.value.clone());

                parameters
            })
    }
}

pub struct Events;

impl Events {
    pub fn new(logs: &Vec<Log>, contracts: &Vec<Contract>) -> Vec<Event> {
        let events_by_topics = Contracts::group_events_by_topics(contracts);

        logs.iter()
            .map(|log| Event::new(log, &events_by_topics.get(&log.topics[0]).unwrap()))
            .collect()
    }
}