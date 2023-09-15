use chaindexing::EventsIngesterJsonRpc;
use ethers::providers::ProviderError;
use ethers::types::{Filter, Log, U64};

pub fn empty_json_rpc() -> impl EventsIngesterJsonRpc {
    #[derive(Clone)]
    struct JsonRpc;
    #[async_trait::async_trait]
    impl EventsIngesterJsonRpc for JsonRpc {
        async fn get_block_number(&self) -> Result<U64, ProviderError> {
            Ok(U64::from(0))
        }

        async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
            Ok(vec![])
        }
    }

    return JsonRpc;
}

#[macro_export]
macro_rules! json_rpc_with_logs {
    ($contract_address:expr) => {{
        use crate::json_rpc_with_logs;

        json_rpc_with_logs!($contract_address, 3)
    }};
    ($contract_address:expr, $current_block_number:expr) => {{
        use chaindexing::EventsIngesterJsonRpc;
        use ethers::providers::ProviderError;
        use ethers::types::{Bytes, Filter, Log, H160, H256, U64};
        use std::str::FromStr;

        #[derive(Clone)]
        struct JsonRpc;
        #[async_trait::async_trait]
        impl EventsIngesterJsonRpc for JsonRpc {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from($current_block_number))
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![Log {
                    address: H160::from_str($contract_address).unwrap(),
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
                    log_index: Some(218.into()),
                    transaction_log_index: None,
                    log_type: None,
                    removed: Some(false),
                }])
            }
        }

        fn h256(str: &str) -> H256 {
            H256::from_str(str).unwrap()
        }

        JsonRpc
    }};
}

#[macro_export]
macro_rules! json_rpc_with_filter_stubber {
    ($contract_address:expr, $filter_stubber: expr) => {{
        use chaindexing::EventsIngesterJsonRpc;
        use ethers::providers::ProviderError;
        use ethers::types::{Filter, Log};

        #[derive(Clone)]
        struct JsonRpc;
        #[async_trait::async_trait]
        impl EventsIngesterJsonRpc for JsonRpc {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from(3))
            }

            async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                ($filter_stubber)(filter);

                Ok(vec![])
            }
        }

        JsonRpc
    }};
}

#[macro_export]
macro_rules! json_rpc_with_empty_logs {
    ($contract_address:expr) => {{
        use chaindexing::EventsIngesterJsonRpc;
        use ethers::providers::ProviderError;
        use ethers::types::{Filter, Log};

        #[derive(Clone)]
        struct JsonRpc;
        #[async_trait::async_trait]
        impl EventsIngesterJsonRpc for JsonRpc {
            async fn get_block_number(&self) -> Result<U64, ProviderError> {
                Ok(U64::from(3))
            }

            async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ProviderError> {
                Ok(vec![])
            }
        }

        JsonRpc
    }};
}
