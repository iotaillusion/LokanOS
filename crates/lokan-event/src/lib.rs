use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

/// Structured event emitted by services and the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub topic: String,
    pub payload: serde_json::Value,
    pub timestamp: SystemTime,
}

impl Event {
    pub fn new(topic: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            topic: topic.into(),
            payload,
            timestamp: SystemTime::now(),
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: Event) {
        if let Err(err) = self.sender.send(event) {
            warn!(error = %err, "failed to publish event");
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Blocks until the event bus becomes idle for the provided duration.
    pub async fn drain(&self, idle_timeout: Duration) {
        let mut rx = self.sender.subscribe();
        loop {
            match tokio::time::timeout(idle_timeout, rx.recv()).await {
                Ok(Ok(_)) => continue,
                Ok(Err(_)) | Err(_) => break,
            }
        }
    }
}
