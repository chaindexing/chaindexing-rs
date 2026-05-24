use chaindexing::IngesterProvider;
use ethers::providers::ProviderError;
use ethers::types::{Block, Filter, Log, TxHash, H256, U64};

use rand::prelude::*;

pub fn empty_provider() -> impl IngesterProvider {
    #[derive(Clone)]
    struct Provider;
    #[chaindexing::augmenting_std::async_trait]
    impl IngesterProvider for Provider {
        async fn get_block_number(&self) -> Result<U64, ProviderError> {
            Ok(U64::from(0))
        }

        async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
            Ok(Block {
                number: Some(block_number),
                hash: Some(block_hash_for_number(block_number)),
                parent_hash: block_hash_for_number(U64::from(
                    block_number.as_u64().saturating_sub(1),
                )),
                timestamp: block_number.as_u64().into(),
                ..Default::default()
            })
        }
    }

    Provider
}

use ethers::types::{Address, Bytes, ValueOrArray, H160};
use std::str::FromStr;

pub fn filter_matches_contract_address(filter: &Filter, contract_address: &str) -> bool {
    let contract_address = Address::from_str(contract_address).unwrap();

    matches!(
        filter.address.as_ref(),
        Some(ValueOrArray::Value(address)) if *address == contract_address
    )
}

pub fn transfer_log(contract_address: &str) -> Log {
    let log_index = *(1..800).collect::<Vec<_>>().choose(&mut rand::rng()).unwrap();

    Log {
        address: H160::from_str(contract_address).unwrap(),
        topics: vec![
            h256("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"),
            h256("0x000000000000000000000000b518b3136e491101f22b77f385fe22269c515188"),
            h256("0x0000000000000000000000007dfd6013cf8d92b751e63d481b51fe0e4c5abf5e"),
            h256("0x000000000000000000000000000000000000000000000000000000000000067d"),
        ],
        data: Bytes("0x".into()),
        block_hash: Some(h256(
            "0x8fd4ca304a2e81854059bc3e42f32064cca8b6b453f6286f95060edc6382c6f8",
        )),
        block_number: Some(18115958.into()),
        transaction_hash: Some(h256(
            "0x83d751998ff98cd609bc9b18bb36bdef8659cde2f74d6d7a1b0fef2c2bf8f839",
        )),
        transaction_index: Some(89.into()),
        log_index: Some(log_index.into()),
        transaction_log_index: None,
        log_type: None,
        removed: Some(false),
    }
}

fn h256(str: &str) -> H256 {
    H256::from_str(str).unwrap()
}

pub fn block_hash_for_number(block_number: U64) -> H256 {
    H256::from_low_u64_be(block_number.as_u64())
}

pub fn block_number_for_hash(block_hash: H256) -> U64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&block_hash.as_bytes()[24..32]);
    U64::from(u64::from_be_bytes(bytes))
}

#[macro_export]
macro_rules! provider_with_logs {
    ($contract_address:expr) => {{
        use $crate::provider_with_logs;

        provider_with_logs!($contract_address, 17774490)
    }};
    ($contract_address:expr, $current_block_number:expr) => {{
        use chaindexing::IngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};
        use $crate::factory::{
            block_hash_for_number, block_number_for_hash, filter_matches_contract_address,
            transfer_log,
        };

        #[derive(Clone)]
        struct Provider {
            contract_address: String,
        }
        #[chaindexing::augmenting_std::async_trait]
        impl IngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from($current_block_number))
            }

            async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                if !filter_matches_contract_address(filter, &self.contract_address) {
                    return Ok(vec![]);
                }

                let mut log = transfer_log(&self.contract_address);
                if let Some(block_hash) = filter.get_block_hash() {
                    log.block_hash = Some(block_hash);
                    log.block_number = Some(block_number_for_hash(block_hash));
                } else {
                    let block_number = filter.get_from_block().unwrap_or_else(|| U64::from(0));
                    log.block_hash = Some(block_hash_for_number(block_number));
                    log.block_number = Some(block_number);
                }

                Ok(vec![log])
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    hash: Some(block_hash_for_number(block_number)),
                    parent_hash: block_hash_for_number(U64::from(
                        block_number.as_u64().saturating_sub(1),
                    )),
                    timestamp: block_number.as_u64().into(),
                    ..Default::default()
                })
            }
        }

        Provider {
            contract_address: $contract_address.to_string(),
        }
    }};
}

#[macro_export]
macro_rules! provider_with_filter_stubber {
    ($contract_address:expr, $filter_stubber: expr) => {{
        provider_with_filter_stubber!($contract_address, 3, $filter_stubber)
    }};
    ($contract_address:expr, $current_block_number:expr, $filter_stubber: expr) => {{
        use chaindexing::IngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};
        use $crate::factory::{block_hash_for_number, filter_matches_contract_address};

        #[derive(Clone)]
        struct Provider<FilterStubber> {
            contract_address: String,
            filter_stubber: FilterStubber,
        }
        #[chaindexing::augmenting_std::async_trait]
        impl<FilterStubber> IngesterProvider for Provider<FilterStubber>
        where
            FilterStubber: Fn(&Filter) + Clone + Send + Sync,
        {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from($current_block_number))
            }

            async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                if filter_matches_contract_address(filter, &self.contract_address) {
                    (self.filter_stubber)(filter);
                }

                Ok(vec![])
            }

            fn supports_block_hash_log_filters(&self) -> bool {
                false
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    hash: Some(block_hash_for_number(block_number)),
                    parent_hash: block_hash_for_number(U64::from(
                        block_number.as_u64().saturating_sub(1),
                    )),
                    timestamp: block_number.as_u64().into(),
                    ..Default::default()
                })
            }
        }

        Provider {
            contract_address: $contract_address.to_string(),
            filter_stubber: $filter_stubber,
        }
    }};
}

#[macro_export]
macro_rules! provider_with_empty_logs {
    ($contract_address:expr) => {{
        use chaindexing::IngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};
        use $crate::factory::block_hash_for_number;

        #[derive(Clone)]
        struct Provider;
        #[chaindexing::augmenting_std::async_trait]
        impl IngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from(3))
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![])
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    hash: Some(block_hash_for_number(block_number)),
                    parent_hash: block_hash_for_number(U64::from(
                        block_number.as_u64().saturating_sub(1),
                    )),
                    timestamp: block_number.as_u64().into(),
                    ..Default::default()
                })
            }
        }

        Provider
    }};
}
