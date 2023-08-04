use std::collections::HashMap;

pub use ethers::prelude::Chain;

#[derive(Clone)]
pub struct JsonRpcUrl(pub String);

pub type Chains = HashMap<Chain, JsonRpcUrl>;
