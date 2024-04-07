use std::sync::Arc;

use futures_core::Stream;
use tokio::sync::Mutex;

use contracts::ContractAddress;
use events::Event;

pub trait Streamable {
    type StreamConn<'a>;
    fn get_contract_addresses_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a>;
    fn get_contract_addresses_stream_by_chain<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
        chain_id_: i64,
    ) -> Box<dyn Stream<Item = Vec<ContractAddress>> + Send + Unpin + 'a>;
    fn get_events_stream<'a>(
        conn: Arc<Mutex<Self::StreamConn<'a>>>,
        from: i64,
        chain_id_: i64,
        contract_address_: String,
    ) -> Box<dyn Stream<Item = Vec<Event>> + Send + Unpin + 'a>;
}
