use chaindexing::{Chain, Contract};

use super::{ApprovalForAllTestEventHandler, TestContractState, TransferTestEventHandler};

pub const TRANSFER_EVENT_ABI: &str =
    "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)";

pub const APPROCAL_EVENT_ABI: &str =
    "event ApprovalForAll(address indexed owner, address indexed operator, bool approved)";

pub const BAYC_CONTRACT_ADDRESS: &str = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
pub const BAYC_CONTRACT_START_BLOCK_NUMBER: u32 = 17773490;
pub fn bayc_contract() -> Contract<TestContractState> {
    Contract::new("BoredApeYachtClub")
        .add_event(TRANSFER_EVENT_ABI, TransferTestEventHandler)
        .add_event(APPROCAL_EVENT_ABI, ApprovalForAllTestEventHandler)
        .add_address(BAYC_CONTRACT_ADDRESS, &Chain::Mainnet, 17773490)
}
