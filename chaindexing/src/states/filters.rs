use std::{collections::HashMap, fmt::Debug};

use crate::Event;

#[derive(Clone, Debug)]
enum FiltersContext {
    Contract,
    Chain,
    MultiChain,
}

/// Represents a set of filters used for querying data.
#[derive(Clone, Debug)]
pub struct Filters {
    values: HashMap<String, String>, // A map of filter field names to their values.
    context: FiltersContext,         // The context in which the filters are applied.
}

impl Filters {
    /// Creates a new Filters instance with a single filter.
    ///
    /// # Arguments
    ///
    /// * `field` - The field name of the filter.
    /// * `value` - The value of the filter.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let filters = Filters::new("token_id", token_id);
    /// Nft::read_one(&filters, &context);
    /// ```
    pub fn new(field: impl ToString, value: impl ToString) -> Self {
        Self {
            values: HashMap::from([(field.to_string(), value.to_string())]),
            context: FiltersContext::Contract,
        }
    }

    /// Adds a new filter to the existing set of filters by moving the
    /// original filters
    ///
    /// # Arguments
    ///
    /// * `field` - The field name of the filter.
    /// * `value` - The value of the filter.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Filters::new("address", address).add("token_id", token_id); // filters is moved
    /// ```
    pub fn add(mut self, field: impl ToString, value: impl ToString) -> Self {
        self.add_mut(field, value);
        self
    }

    /// Adds a new filter to the existing set of filters without moving
    /// the original filters
    ///
    /// # Arguments
    ///
    /// * `field` - The field name of the filter.
    /// * `value` - The value of the filter.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut filters = Filters::new("address", address);
    ///
    /// filters.add_mut("token_id", token_id); // filters not moved
    /// ```
    pub fn add_mut(&mut self, field: impl ToString, value: impl ToString) {
        self.values.insert(field.to_string(), value.to_string());
    }

    /// Sets the context of the filters to Contract
    pub fn within_contract(mut self) -> Self {
        self.context = FiltersContext::Contract;
        self
    }

    /// Sets the context of the filters to Chain
    pub fn within_chain(mut self) -> Self {
        self.context = FiltersContext::Chain;
        self
    }

    /// Sets the context of the filters to MultiChain
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
