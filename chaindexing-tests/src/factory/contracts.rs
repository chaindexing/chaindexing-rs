use chaindexing::{ChainId, Contract};

use super::{ApprovalForAllTestHandler, TransferTestHandler};

pub const BAYC_CONTRACT_ADDRESS: &str = "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D";
pub const BAYC_CONTRACT_START_BLOCK_NUMBER: u32 = 17773490;

pub fn contract_address_for_seed(seed: &str) -> String {
    let mut bytes = [0_u8; 20];

    for (index, byte) in seed.bytes().enumerate() {
        let slot = index % bytes.len();
        bytes[slot] = bytes[slot].wrapping_add(byte).wrapping_add((index as u8).wrapping_mul(31));
    }

    let hex = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("0x{hex}")
}

pub fn bayc_contract(name: &str, two_digit_nonce: &str) -> Contract<()> {
    Contract::new(name)
        .add_event_handler(TransferTestHandler)
        .add_event_handler(ApprovalForAllTestHandler)
        .add_address(
            &format!("0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f{two_digit_nonce}D"),
            &ChainId::Mainnet,
            17773490,
        )
}

pub fn bayc_contract_with_address_seed(name: &str, seed: &str) -> Contract<()> {
    Contract::new(name)
        .add_event_handler(TransferTestHandler)
        .add_event_handler(ApprovalForAllTestHandler)
        .add_address(
            &contract_address_for_seed(seed),
            &ChainId::Mainnet,
            17773490,
        )
}
