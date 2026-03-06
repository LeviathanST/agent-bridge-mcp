use tokio::sync::broadcast;

use crate::models::Message;

/// In-process fan-out for real-time message delivery.
#[derive(Clone)]
pub struct Hub {
    tx: broadcast::Sender<Message>,
}

impl Hub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Hub { tx }
    }

    /// Publish a message to all subscribers. Safe to call even if no one is listening.
    pub fn publish(&self, msg: Message) {
        let _ = self.tx.send(msg);
    }

    /// Get a new receiver for incoming messages.
    pub fn subscribe(&self) -> broadcast::Receiver<Message> {
        self.tx.subscribe()
    }
}
