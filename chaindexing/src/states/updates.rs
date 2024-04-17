use std::{collections::HashMap, fmt::Debug};

#[derive(Clone, Debug)]
pub struct Updates {
    pub(super) values: HashMap<String, String>,
}

impl Updates {
    pub fn new(field: impl ToString, value: impl ToString) -> Self {
        Self {
            values: HashMap::from([(field.to_string(), value.to_string())]),
        }
    }
    pub fn add(mut self, field: impl ToString, value: impl ToString) -> Self {
        self.values.insert(field.to_string(), value.to_string());
        self
    }
}
