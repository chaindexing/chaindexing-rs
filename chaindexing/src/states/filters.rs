use std::{collections::HashMap, fmt::Debug};

use crate::Event;

#[derive(Clone, Debug)]
enum FiltersContext {
    Contract,
    Chain,
    MultiChain,
}

#[derive(Clone, Debug)]
pub struct Filters {
    values: HashMap<String, String>,
    context: FiltersContext,
}

impl Filters {
    pub fn new(field: impl ToString, value: impl ToString) -> Self {
        Self {
            values: HashMap::from([(field.to_string(), value.to_string())]),
            context: FiltersContext::MultiChain,
        }
    }
    pub fn add(mut self, field: impl ToString, value: impl ToString) -> Self {
        self.values.insert(field.to_string(), value.to_string());
        self
    }
    pub fn within_contract(mut self) -> Self {
        self.context = FiltersContext::Contract;
        self
    }
    pub fn within_chain(mut self) -> Self {
        self.context = FiltersContext::Chain;
        self
    }
    pub fn within_multi_chain(mut self) -> Self {
        self.context = FiltersContext::MultiChain;
        self
    }
    pub(super) fn get(&self, event: &Event) -> HashMap<String, String> {
        let mut filters = self.values.clone();

        match self.context {
            FiltersContext::Contract => {
                filters.insert("chain_id".to_string(), event.chain_id.to_string());
                filters.insert(
                    "contract_address".to_string(),
                    event.contract_address.to_owned(),
                );
            }
            FiltersContext::Chain => {
                filters.insert("chain_id".to_string(), event.chain_id.to_string());
            }
            FiltersContext::MultiChain => {}
        }

        filters
    }
}
