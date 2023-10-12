use chaindexing::{Contract, Event, Events};

use super::{transfer_log, BAYC_CONTRACT_ADDRESS};

pub fn transfer_event_with_contract(contract: Contract) -> Event {
    let contract_address = BAYC_CONTRACT_ADDRESS;
    let transfer_log = transfer_log(contract_address);

    Events::new(&vec![transfer_log], &vec![contract]).first().cloned().unwrap()
}
