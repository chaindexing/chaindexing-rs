use std::collections::HashMap;

#[cfg(feature = "live-states")]
use once_cell::sync::Lazy;
#[cfg(feature = "live-states")]
use tokio::sync::broadcast;

#[cfg(feature = "live-states")]
pub type Message = crate::StateChange;

#[cfg(feature = "live-states")]
static BUS: Lazy<broadcast::Sender<Message>> = Lazy::new(|| broadcast::channel(1_024).0);

#[cfg(feature = "live-states")]
pub(crate) fn publish(msg: Message) {
    // Ignore lagging-receiver errors – they only affect that receiver
    let _ = BUS.send(msg);
}

#[cfg(feature = "live-states")]
pub fn subscribe() -> broadcast::Receiver<Message> {
    BUS.subscribe()
}

// No-op implementations when feature is disabled
#[cfg(not(feature = "live-states"))]
pub(crate) fn publish(_msg: crate::StateChange) {
    // No-op when feature is disabled
}
