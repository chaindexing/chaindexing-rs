use crate::{ChaindexingRepoRawQueryTxnClient, Event};

pub trait HandlerContext<'a>: Send + Sync {
    fn get_event(&self) -> &Event;
    fn get_raw_query_client(&self) -> &ChaindexingRepoRawQueryTxnClient<'a>;
}
