use chaindexing::{ChainId, Contract};

use super::{ApprovalForAllTestEventHandler, TransferTestEventHandler};

pub const BAYC_CONTRACT_ADDRESS: &str = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
pub const BAYC_CONTRACT_START_BLOCK_NUMBER: u32 = 17773490;
pub fn bayc_contract() -> Contract<()> {
    Contract::new("BoredApeYachtClub")
        .add_event_handler(TransferTestEventHandler)
        .add_event_handler(ApprovalForAllTestEventHandler)
        .add_address(BAYC_CONTRACT_ADDRESS, &ChainId::Mainnet, 17773490)
}
