//! Message bus abstractions shared across services.

use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures::Stream;
use thiserror::Error;
use tokio::time::Duration;

/// Represents a message that traveled across the bus.
#[derive(Debug, Clone)]
pub struct BusMessage {
    /// Subject/topic the message was published on.
    pub subject: String,
    /// Message payload bytes.
    pub payload: Vec<u8>,
    /// Optional reply subject to send a response to.
    pub reply: Option<String>,
}

impl BusMessage {
    /// Convenience helper to respond to a request using the provided bus instance.
    pub async fn respond<B: MessageBus + ?Sized>(
        &self,
        bus: &B,
        payload: &[u8],
    ) -> Result<(), MsgBusError> {
        if let Some(reply) = &self.reply {
            bus.respond(reply, payload).await
        } else {
            Err(MsgBusError::MissingReplySubject)
        }
    }
}

/// Stream of [`BusMessage`] items for a given subscription.
pub struct Subscription {
    subject: String,
    inner: Pin<Box<dyn Stream<Item = BusMessage> + Send>>, // boxed stream for dynamic dispatch
}

impl Subscription {
    fn new<S>(subject: String, stream: S) -> Self
    where
        S: Stream<Item = BusMessage> + Send + 'static,
    {
        Self {
            subject,
            inner: Box::pin(stream),
        }
    }

    /// Subject this subscription listens on.
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

impl Stream for Subscription {
    type Item = BusMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // SAFETY: `inner` is pinned inside the struct and never moved after construction.
        unsafe {
            let inner = self.map_unchecked_mut(|me| &mut me.inner);
            inner.poll_next(cx)
        }
    }
}

/// Errors that can occur while interacting with the message bus layer.
#[derive(Debug, Error)]
pub enum MsgBusError {
    /// Underlying client failed to connect to the server.
    #[error("unable to connect to message bus: {0}")]
    Connection(String),
    /// A publish operation failed.
    #[error("unable to publish message: {0}")]
    Publish(String),
    /// A subscribe operation failed.
    #[error("unable to create subscription: {0}")]
    Subscribe(String),
    /// Request/response exchange failed.
    #[error("request failed: {0}")]
    Request(String),
    /// Request timed out waiting for a response.
    #[error("request timed out after {0:?}")]
    RequestTimeout(Duration),
    /// Attempted to respond to a message that lacked a reply subject.
    #[error("message did not include a reply subject")]
    MissingReplySubject,
    /// Attempted to reply to an unknown subject.
    #[error("no pending request for reply subject {0}")]
    UnknownReplySubject(String),
}

/// Abstraction over the platform message bus backend.
#[async_trait]
pub trait MessageBus: Send + Sync {
    /// Publish `payload` to the given `subject`.
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), MsgBusError>;

    /// Subscribe to the given `subject`, returning a stream of [`BusMessage`].
    async fn subscribe(&self, subject: &str) -> Result<Subscription, MsgBusError>;

    /// Send a request and wait for the response.
    async fn request(&self, subject: &str, payload: &[u8]) -> Result<BusMessage, MsgBusError>;

    /// Respond to a request using the provided reply subject.
    async fn respond(&self, reply_to: &str, payload: &[u8]) -> Result<(), MsgBusError>;
}

#[cfg(feature = "nats")]
mod nats_impl {
    use super::{BusMessage, MessageBus, MsgBusError, Subscription};
    use async_trait::async_trait;
    use futures::StreamExt;
    use tokio::time::{timeout, Duration};

    /// Configuration for establishing a NATS-backed message bus connection.
    #[derive(Debug, Clone)]
    pub struct NatsConfig {
        /// URL pointing to the NATS server instance.
        pub url: String,
        /// How long to wait for request/response exchanges.
        pub request_timeout: Duration,
    }

    impl Default for NatsConfig {
        fn default() -> Self {
            Self {
                url: "nats://127.0.0.1:4222".to_string(),
                request_timeout: Duration::from_secs(2),
            }
        }
    }

    /// Wrapper around an [`async_nats::Client`] implementing [`MessageBus`].
    #[derive(Clone)]
    pub struct NatsBus {
        client: async_nats::Client,
        request_timeout: Duration,
    }

    impl NatsBus {
        /// Establish a new connection to the configured NATS endpoint.
        pub async fn connect(config: NatsConfig) -> Result<Self, MsgBusError> {
            let client = async_nats::connect(config.url.clone())
                .await
                .map_err(|err| MsgBusError::Connection(err.to_string()))?;
            Ok(Self {
                client,
                request_timeout: config.request_timeout,
            })
        }
    }

    #[async_trait]
    impl MessageBus for NatsBus {
        async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), MsgBusError> {
            self.client
                .publish(subject.to_string(), payload.to_vec().into())
                .await
                .map_err(|err| MsgBusError::Publish(err.to_string()))
        }

        async fn subscribe(&self, subject: &str) -> Result<Subscription, MsgBusError> {
            let subject_str = subject.to_string();
            let subscriber = self
                .client
                .subscribe(subject_str.clone())
                .await
                .map_err(|err| MsgBusError::Subscribe(err.to_string()))?;

            let stream = subscriber.map(|message| BusMessage {
                subject: message.subject.to_string(),
                payload: message.payload.to_vec(),
                reply: message.reply.map(|subject| subject.to_string()),
            });
            Ok(Subscription::new(subject_str, stream))
        }

        async fn request(&self, subject: &str, payload: &[u8]) -> Result<BusMessage, MsgBusError> {
            let response = timeout(
                self.request_timeout,
                self.client
                    .request(subject.to_string(), payload.to_vec().into()),
            )
            .await
            .map_err(|_| MsgBusError::RequestTimeout(self.request_timeout))
            .and_then(|result| result.map_err(|err| MsgBusError::Request(err.to_string())))?;

            Ok(BusMessage {
                subject: response.subject.to_string(),
                payload: response.payload.to_vec(),
                reply: response.reply.map(|subject| subject.to_string()),
            })
        }

        async fn respond(&self, reply_to: &str, payload: &[u8]) -> Result<(), MsgBusError> {
            self.client
                .publish(reply_to.to_string(), payload.to_vec().into())
                .await
                .map_err(|err| MsgBusError::Publish(err.to_string()))
        }
    }

    pub use NatsBus as Client;
    pub use NatsConfig as Config;
}

#[cfg(feature = "nats")]
pub use nats_impl::{Client as NatsBus, Config as NatsConfig};

#[cfg(feature = "mock")]
mod mock_impl {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };

    use async_trait::async_trait;
    use dashmap::DashMap;
    use futures::StreamExt;
    use tokio::sync::{broadcast, oneshot};
    use tokio_stream::wrappers::BroadcastStream;

    use super::{BusMessage, MessageBus, MsgBusError, Subscription};

    const CHANNEL_CAPACITY: usize = 64;

    fn ensure_subject(
        map: &DashMap<String, broadcast::Sender<BusMessage>>,
        subject: &str,
    ) -> broadcast::Sender<BusMessage> {
        if let Some(existing) = map.get(subject) {
            existing.clone()
        } else {
            let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
            match map.entry(subject.to_string()) {
                dashmap::mapref::entry::Entry::Occupied(entry) => entry.get().clone(),
                dashmap::mapref::entry::Entry::Vacant(entry) => entry.insert(tx).clone(),
            }
        }
    }

    /// In-process mock message bus used for unit/integration tests.
    #[derive(Clone, Default)]
    pub struct MockBus {
        subjects: Arc<DashMap<String, broadcast::Sender<BusMessage>>>,
        pending: Arc<DashMap<String, oneshot::Sender<Vec<u8>>>>,
        request_counter: Arc<AtomicU64>,
    }

    impl MockBus {
        /// Create a new instance of the mock bus.
        pub fn new() -> Self {
            Self::default()
        }

        fn next_reply_subject(&self) -> String {
            let id = self.request_counter.fetch_add(1, Ordering::Relaxed);
            format!("inproc.reply.{}", id)
        }
    }

    #[async_trait]
    impl MessageBus for MockBus {
        async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), MsgBusError> {
            let sender = ensure_subject(&self.subjects, subject);
            let message = BusMessage {
                subject: subject.to_string(),
                payload: payload.to_vec(),
                reply: None,
            };
            sender
                .send(message)
                .map(|_| ())
                .map_err(|err| MsgBusError::Publish(err.to_string()))
        }

        async fn subscribe(&self, subject: &str) -> Result<Subscription, MsgBusError> {
            let sender = ensure_subject(&self.subjects, subject);
            let receiver = sender.subscribe();
            let subject_string = subject.to_string();
            let stream = BroadcastStream::new(receiver).filter_map(|item| match item {
                Ok(message) => Some(message),
                Err(broadcast::error::RecvError::Lagged(_)) => None,
                Err(_) => None,
            });
            Ok(Subscription::new(subject_string, stream))
        }

        async fn request(&self, subject: &str, payload: &[u8]) -> Result<BusMessage, MsgBusError> {
            let (tx, rx) = oneshot::channel();
            let reply_subject = self.next_reply_subject();
            self.pending.insert(reply_subject.clone(), tx);

            let sender = ensure_subject(&self.subjects, subject);
            let message = BusMessage {
                subject: subject.to_string(),
                payload: payload.to_vec(),
                reply: Some(reply_subject.clone()),
            };
            sender
                .send(message)
                .map_err(|err| MsgBusError::Request(err.to_string()))?;

            let payload = rx
                .await
                .map_err(|err| MsgBusError::Request(err.to_string()))?;

            Ok(BusMessage {
                subject: reply_subject,
                payload,
                reply: None,
            })
        }

        async fn respond(&self, reply_to: &str, payload: &[u8]) -> Result<(), MsgBusError> {
            if let Some((_, waiter)) = self.pending.remove(reply_to) {
                waiter
                    .send(payload.to_vec())
                    .map_err(|_| MsgBusError::Request("pending request dropped".to_string()))
            } else {
                // fall back to publish semantics if no pending request exists
                let sender = ensure_subject(&self.subjects, reply_to);
                let message = BusMessage {
                    subject: reply_to.to_string(),
                    payload: payload.to_vec(),
                    reply: None,
                };
                sender
                    .send(message)
                    .map(|_| ())
                    .map_err(|_| MsgBusError::UnknownReplySubject(reply_to.to_string()))
            }
        }
    }

    pub use MockBus as Client;
}

#[cfg(feature = "mock")]
pub use mock_impl::Client as MockBus;

#[cfg(all(test, feature = "mock"))]
mod tests {
    use futures::StreamExt;

    use super::{MessageBus, MockBus};

    #[tokio::test]
    async fn mock_bus_round_trip() {
        let bus = MockBus::new();
        let mut subscription = bus.subscribe("test.subject").await.expect("subscribe");

        let handle = tokio::spawn({
            let bus = bus.clone();
            async move {
                if let Some(message) = subscription.next().await {
                    assert_eq!(message.subject, "test.subject");
                    assert_eq!(message.payload, b"ping");
                    message.respond(&bus, b"pong").await.expect("respond");
                }
            }
        });

        let response = bus.request("test.subject", b"ping").await.expect("request");

        assert_eq!(response.payload, b"pong");
        handle.await.expect("subscription task");
    }
}
