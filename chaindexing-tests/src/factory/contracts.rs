use chaindexing::{ChainId, Contract};

use super::{ApprovalForAllTestHandler, TransferTestHandler};

pub const BAYC_CONTRACT_ADDRESS: &str = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
pub const BAYC_CONTRACT_START_BLOCK_NUMBER: u32 = 17773490;
pub fn bayc_contract(name: &str, two_digit_nonce: &str) -> Contract<()> {
    Contract::new(name)
        .add_handler(TransferTestHandler)
        .add_handler(ApprovalForAllTestHandler)
        .add_address(
            &format!("0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f{two_digit_nonce}D"),
            &ChainId::Mainnet,
            17773490,
        )
}
