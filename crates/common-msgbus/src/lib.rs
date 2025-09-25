//! Message bus abstractions shared across services.

use thiserror::Error;

/// Errors that can occur while interacting with the message bus layer.
#[derive(Debug, Error)]
pub enum MsgBusError {
    /// Stub error variant for unimplemented operations.
    #[error("message bus operation not implemented")]
    NotImplemented,
}

/// Represents a message published onto the platform bus.
#[derive(Debug, Clone)]
pub struct BusMessage {
    pub topic: String,
    pub payload: Vec<u8>,
}

/// Publish a message to the shared bus.
///
/// Stub implementation that only logs the outgoing event.
#[allow(unused_variables)]
pub fn publish(message: &BusMessage) -> Result<(), MsgBusError> {
    tracing::info!(topic = %message.topic, payload_len = message.payload.len(), "publishing stub message");
    Ok(())
}
