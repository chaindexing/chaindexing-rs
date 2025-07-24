use chaindexing::{ChainId, Contract, ContractEvent, Event};
use ethers::types::{Bytes, Log, H160, H256};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{transfer_log, BAYC_CONTRACT_ADDRESS};

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

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

pub fn unique_transfer_event_with_contract(contract: Contract<()>) -> Event {
    let contract_address = contract.addresses.first().unwrap().address.as_str();
    let transfer_log = unique_transfer_log_with_contract_name(contract_address, &contract.name);

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

pub fn unique_transfer_log_with_contract_name(contract_address: &str, contract_name: &str) -> Log {
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Get a unique counter value for this call
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::SeqCst);

    // Hash the contract name to add more uniqueness
    let mut hasher = DefaultHasher::new();
    contract_name.hash(&mut hasher);
    let contract_hash = hasher.finish();

    // Use current timestamp, process ID, contract hash, and counter to ensure uniqueness
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;

    let process_id = process::id() as u64;

    // Generate unique values based on timestamp, process ID, contract hash, and counter
    let unique_seed = timestamp
        .wrapping_mul(process_id)
        .wrapping_add(contract_hash)
        .wrapping_add(counter);
    let log_index = (unique_seed % 1000) + 1;
    let block_number = 18_000_000 + (unique_seed % 1_000_000);
    let transaction_index = (unique_seed % 200) + 1;

    // Generate unique hashes based on the unique seed
    let mut tx_hash_bytes = [0u8; 32];
    let tx_seed = unique_seed.wrapping_mul(3141592653);
    for (i, byte) in tx_hash_bytes.iter_mut().enumerate() {
        *byte = ((tx_seed.wrapping_add(i as u64)) % 256) as u8;
    }
    let transaction_hash = H256::from(tx_hash_bytes);

    let mut block_hash_bytes = [0u8; 32];
    let block_seed = unique_seed.wrapping_mul(2718281828);
    for (i, byte) in block_hash_bytes.iter_mut().enumerate() {
        *byte = ((block_seed.wrapping_add(i as u64)) % 256) as u8;
    }
    let block_hash = H256::from(block_hash_bytes);

    Log {
        address: H160::from_str(contract_address).unwrap(),
        topics: vec![
            h256("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"),
            h256("0x000000000000000000000000b518b3136e491101f22b77f385fe22269c515188"),
            h256("0x0000000000000000000000007dfd6013cf8d92b751e63d481b51fe0e4c5abf5e"),
            h256("0x000000000000000000000000000000000000000000000000000000000000067d"),
        ],
        data: Bytes("0x".into()),
        block_hash: Some(block_hash),
        block_number: Some(block_number.into()),
        transaction_hash: Some(transaction_hash),
        transaction_index: Some(transaction_index.into()),
        log_index: Some(log_index.into()),
        transaction_log_index: Some(log_index.into()),
        log_type: None,
        removed: Some(false),
    }
}

fn h256(str: &str) -> H256 {
    H256::from_str(str).unwrap()
}
