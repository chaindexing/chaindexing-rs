use chaindexing::{Event, EventHandler};

pub struct TestEventHandler;

#[async_trait::async_trait]
impl EventHandler for TestEventHandler {
    async fn handle_event(&self, _event: Event) {}
}
