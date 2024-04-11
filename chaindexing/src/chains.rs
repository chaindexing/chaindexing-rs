pub type ChainId = ethers::types::Chain;
#[derive(Clone, Debug)]
pub struct Chain {
    pub id: ChainId,
    pub json_rpc_url: String,
}

impl Chain {
    pub fn new(id: ChainId, json_rpc_url: &str) -> Self {
        Self {
            id,
            json_rpc_url: json_rpc_url.to_string(),
        }
    }
}
