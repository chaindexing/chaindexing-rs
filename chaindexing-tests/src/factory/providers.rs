use alloy::consensus::Header as ConsensusHeader;
use alloy::primitives::{Address, Bytes, Log as PrimitiveLog, LogData, B256};
use alloy::rpc::types::{Block, Filter, Header as RpcHeader, Log};
use chaindexing::ingester::ProviderError;
use chaindexing::IngesterProvider;

use rand::prelude::*;

pub fn empty_provider() -> impl IngesterProvider {
    #[derive(Clone)]
    struct Provider;
    #[chaindexing::augmenting_std::async_trait]
    impl IngesterProvider for Provider {
        async fn get_block_number(&self) -> Result<u64, ProviderError> {
            Ok(0)
        }

        async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
            Ok(block_for_number(block_number))
        }
    }

    Provider
}

use std::str::FromStr;

pub fn filter_matches_contract_address(filter: &Filter, contract_address: &str) -> bool {
    let contract_address = Address::from_str(contract_address).unwrap();

    filter.matches_address(contract_address)
}

pub fn transfer_log(contract_address: &str) -> Log {
    let log_index = *(1..800).collect::<Vec<_>>().choose(&mut rand::rng()).unwrap();

    Log {
        inner: PrimitiveLog {
            address: Address::from_str(contract_address).unwrap(),
            data: LogData::new_unchecked(
                vec![
                    h256("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"),
                    h256("0x000000000000000000000000b518b3136e491101f22b77f385fe22269c515188"),
                    h256("0x0000000000000000000000007dfd6013cf8d92b751e63d481b51fe0e4c5abf5e"),
                    h256("0x000000000000000000000000000000000000000000000000000000000000067d"),
                ],
                Bytes::new(),
            ),
        },
        block_hash: Some(h256(
            "0x8fd4ca304a2e81854059bc3e42f32064cca8b6b453f6286f95060edc6382c6f8",
        )),
        block_number: Some(18115958),
        transaction_hash: Some(h256(
            "0x83d751998ff98cd609bc9b18bb36bdef8659cde2f74d6d7a1b0fef2c2bf8f839",
        )),
        transaction_index: Some(89),
        log_index: Some(log_index),
        removed: false,
        ..Default::default()
    }
}

fn h256(str: &str) -> B256 {
    B256::from_str(str).unwrap()
}

pub fn block_hash_for_number(block_number: u64) -> B256 {
    let mut bytes = [0_u8; 32];
    bytes[24..].copy_from_slice(&block_number.to_be_bytes());
    B256::from(bytes)
}

pub fn block_number_for_hash(block_hash: B256) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&block_hash.as_slice()[24..32]);
    u64::from_be_bytes(bytes)
}

pub fn block_for_number(block_number: u64) -> Block {
    Block {
        header: RpcHeader {
            hash: block_hash_for_number(block_number),
            inner: ConsensusHeader {
                number: block_number,
                parent_hash: block_hash_for_number(block_number.saturating_sub(1)),
                timestamp: block_number,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

#[macro_export]
macro_rules! provider_with_logs {
    ($contract_address:expr) => {{
        use $crate::provider_with_logs;

        provider_with_logs!($contract_address, 17774490)
    }};
    ($contract_address:expr, $current_block_number:expr) => {{
        use alloy::rpc::types::{Block, Filter, Log};
        use chaindexing::ingester::ProviderError;
        use chaindexing::IngesterProvider;
        use $crate::factory::{
            block_for_number, block_hash_for_number, block_number_for_hash,
            filter_matches_contract_address, transfer_log,
        };

        #[derive(Clone)]
        struct Provider {
            contract_address: String,
        }
        #[chaindexing::augmenting_std::async_trait]
        impl IngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<u64, ProviderError> {
                Ok($current_block_number as u64)
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
                    let block_number = filter.get_from_block().unwrap_or(0);
                    log.block_hash = Some(block_hash_for_number(block_number));
                    log.block_number = Some(block_number);
                }

                Ok(vec![log])
            }

            async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
                Ok(block_for_number(block_number))
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
        use alloy::rpc::types::{Block, Filter, Log};
        use chaindexing::ingester::ProviderError;
        use chaindexing::IngesterProvider;
        use $crate::factory::{block_for_number, filter_matches_contract_address};

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
            async fn get_block_number(&self) -> Result<u64, ProviderError> {
                Ok($current_block_number as u64)
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

            async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
                Ok(block_for_number(block_number))
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
        use $crate::provider_with_empty_logs;

        provider_with_empty_logs!($contract_address, 3)
    }};
    ($contract_address:expr, $current_block_number:expr) => {{
        use alloy::rpc::types::{Block, Filter, Log};
        use chaindexing::ingester::ProviderError;
        use chaindexing::IngesterProvider;
        use $crate::factory::block_for_number;

        #[derive(Clone)]
        struct Provider;
        #[chaindexing::augmenting_std::async_trait]
        impl IngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<u64, ProviderError> {
                Ok($current_block_number as u64)
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![])
            }

            async fn get_block(&self, block_number: u64) -> Result<Block, ProviderError> {
                Ok(block_for_number(block_number))
            }
        }

        Provider
    }};
}
