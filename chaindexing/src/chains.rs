/// Represents the network ID for an EVM Chain
/// For example, `ChainId::Mainnet`, `ChainId::Polygon`, etc.
pub type ChainId = ethers::types::Chain;

/// Represents an EVM chain network
#[derive(Clone, Debug)]
pub struct Chain {
    pub id: ChainId,
    pub json_rpc_url: String,
}

impl Chain {
    /// Builds the chain network
    ///
    ///
    /// # Example
    /// ```
    /// use chaindexing::{Chain, ChainId};
    ///
    /// Chain::new(ChainId::Polygon, "https://polygon-mainnet.g.alchemy.com/v2/...");
    /// ```
    pub fn new(id: ChainId, json_rpc_url: &str) -> Self {
        Self {
            id,
            json_rpc_url: json_rpc_url.to_string(),
        }
    }
}
