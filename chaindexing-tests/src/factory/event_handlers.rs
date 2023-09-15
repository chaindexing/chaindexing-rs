use chaindexing::{Event, EventHandler};

pub struct TestEventHandler;

impl EventHandler for TestEventHandler {
    fn handle_event(_event: Event) {}
}
