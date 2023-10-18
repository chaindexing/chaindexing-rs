use ethers::types::{H160, H256};

pub struct Hashes;

impl Hashes {
    pub fn h160_to_string(h160: &H160) -> String {
        serde_json::to_value(h160).unwrap().as_str().unwrap().to_string()
    }

    pub fn h256_to_string(h256: &H256) -> String {
        serde_json::to_value(h256).unwrap().as_str().unwrap().to_string()
    }
}
