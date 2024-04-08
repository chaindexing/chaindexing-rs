use chaindexing::EventsIngesterProvider;
use ethers::providers::ProviderError;
use ethers::types::{Block, Filter, Log, TxHash, U64};

use rand::seq::SliceRandom;

pub fn empty_provider() -> impl EventsIngesterProvider {
    #[derive(Clone)]
    struct Provider;
    #[async_trait::async_trait]
    impl EventsIngesterProvider for Provider {
        async fn get_block_number(&self) -> Result<U64, ProviderError> {
            Ok(U64::from(0))
        }

        async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }

        async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
            Ok(Block {
                number: Some(block_number),
                ..Default::default()
            })
        }
    }

    Provider
}

use ethers::types::{Bytes, H160, H256};
use std::str::FromStr;

pub fn transfer_log(contract_address: &str) -> Log {
    let log_index = *(1..800).collect::<Vec<_>>().choose(&mut rand::thread_rng()).unwrap();

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

#[macro_export]
macro_rules! provider_with_logs {
    ($contract_address:expr) => {{
        use $crate::provider_with_logs;

        provider_with_logs!($contract_address, 17774490)
    }};
    ($contract_address:expr, $current_block_number:expr) => {{
        use chaindexing::EventsIngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};
        use $crate::factory::transfer_log;

        #[derive(Clone)]
        struct Provider;
        #[async_trait::async_trait]
        impl EventsIngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from($current_block_number))
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![transfer_log($contract_address)])
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    ..Default::default()
                })
            }
        }

        Provider
    }};
}

#[macro_export]
macro_rules! provider_with_filter_stubber {
    ($contract_address:expr, $filter_stubber: expr) => {{
        use chaindexing::EventsIngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};

        #[derive(Clone)]
        struct Provider;
        #[async_trait::async_trait]
        impl EventsIngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from(3))
            }

            async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                let filter_stubber = $filter_stubber;

                filter_stubber(filter);

                Ok(vec![])
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    ..Default::default()
                })
            }
        }

        Provider
    }};
}

#[macro_export]
macro_rules! provider_with_empty_logs {
    ($contract_address:expr) => {{
        use chaindexing::EventsIngesterProvider;
        use ethers::providers::ProviderError;
        use ethers::types::{Block, Filter, Log, TxHash, U64};

        #[derive(Clone)]
        struct Provider;
        #[async_trait::async_trait]
        impl EventsIngesterProvider for Provider {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from(3))
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![])
            }

            async fn get_block(&self, block_number: U64) -> Result<Block<TxHash>, ProviderError> {
                Ok(Block {
                    number: Some(block_number),
                    ..Default::default()
                })
            }
        }

        Provider
    }};
}
