use crate::{ChaindexingRepoTxnClient, Event};

pub trait HandlerContext<'a>: Send + Sync {
    fn get_event(&self) -> &Event;
    fn get_client(&self) -> &ChaindexingRepoTxnClient<'a>;
}
