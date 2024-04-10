use chaindexing::{ChainId, Contract, ContractEvent, Event};

use super::{transfer_log, BAYC_CONTRACT_ADDRESS};

pub fn transfer_event_with_contract(contract: Contract<()>) -> Event {
    let contract_address = BAYC_CONTRACT_ADDRESS;
    let transfer_log = transfer_log(contract_address);

    Event::new(
        &transfer_log,
        &ContractEvent::new(
            "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)",
        ),
        &ChainId::Mainnet,
        &contract.name,
        1_i64,
    )
}
