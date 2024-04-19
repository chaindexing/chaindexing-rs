use std::{collections::HashMap, fmt::Debug};

/// Represents the fields to be updated in a state
#[derive(Clone, Debug)]
pub struct Updates {
    pub(super) values: HashMap<String, String>,
}

impl Updates {
    /// Creates a new Updates instance with a single filter.
    ///
    /// # Arguments
    ///
    /// * `field` - The field name to update.
    /// * `value` - new value of field
    ///
    /// # Example
    ///
    /// ```ignore
    /// let updates = Updates::new("owner_address", token_id);
    /// nft_state.update(&updates, &context).await;
    /// ```
    pub fn new(field: impl ToString, value: impl ToString) -> Self {
        Self {
            values: HashMap::from([(field.to_string(), value.to_string())]),
        }
    }
    /// Adds a new update to the existing set of updates by moving the
    /// original updates
    ///
    /// # Arguments
    ///
    /// * `field` - The field name to update.
    /// * `value` - new value of field
    ///
    /// # Example
    ///
    /// ```ignore
    /// Updates::new("address", address).add("token_id", token_id); // updates is moved
    /// ```
    pub fn add(mut self, field: impl ToString, value: impl ToString) -> Self {
        self.add_mut(field, value);
        self
    }
    /// Adds a new update to the existing set of updates without moving
    /// the original updates
    ///
    /// # Arguments
    ///
    /// * `field` - The field name to update.
    /// * `value` - new value of field
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut updates = Updates::new("address", address);
    ///
    /// updates.add_mut("token_id", token_id);// updates not moved
    /// ```
    pub fn add_mut(&mut self, field: impl ToString, value: impl ToString) {
        self.values.insert(field.to_string(), value.to_string());
    }
}
