use crate::{events::Event, Config};

pub trait EventHandler: Send + Sync {
    fn handle_event(event: Event)
    where
        Self: Sized;
}

pub struct EventHandlers;

impl EventHandlers {
    pub fn start(config: Config) {
        // TODO
    }
}
