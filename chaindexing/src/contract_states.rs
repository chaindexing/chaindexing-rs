use std::fmt::Debug;
pub trait ContractState: Debug + Sync + Send + Clone + 'static {}
