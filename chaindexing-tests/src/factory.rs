use chaindexing::{Event, EventHandler};

pub struct TestEventHandler;

impl EventHandler for TestEventHandler {
    fn handle_event(_event: Event) {}
}

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
