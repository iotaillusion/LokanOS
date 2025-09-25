use std::{any::Any, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;

use crate::LokanConfig;

/// Immutable metadata shared with services when they are started.
#[derive(Clone, Default)]
pub struct ServiceContext {
    config: Arc<LokanConfig>,
    extensions: Arc<HashMap<String, Arc<dyn Any + Send + Sync>>>,
}

impl ServiceContext {
    pub fn new(config: Arc<LokanConfig>) -> Self {
        Self {
            config,
            extensions: Arc::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &LokanConfig {
        &self.config
    }

    /// Retrieve an extension by key, attempting to downcast it to the requested type.
    pub fn get_extension<T: Any + Send + Sync>(&self, key: &str) -> Option<Arc<T>> {
        self.extensions
            .get(key)
            .and_then(|value| value.clone().downcast::<T>().ok())
    }

    /// Attach an extension to the context. Returns a new instance with the extension installed.
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: Arc<dyn Any + Send + Sync>,
    ) -> Self {
        Arc::make_mut(&mut self.extensions).insert(key.into(), value);
        self
    }

    /// Convenience helper to attach a strongly typed extension.
    pub fn with_typed_extension<T: Any + Send + Sync>(
        self,
        key: impl Into<String>,
        value: Arc<T>,
    ) -> Self {
        self.with_extension(key, value)
    }
}

/// Runtime state of an individual service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// Error type returned by services at runtime.
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("failed during initialization: {0}")]
    Initialization(String),
    #[error("service runtime error: {0}")]
    Runtime(String),
    #[error("service shutdown error: {0}")]
    Shutdown(String),
}

/// Trait implemented by all services that run inside the Lokan runtime.
#[async_trait]
pub trait Service: Send + Sync {
    /// Unique identifier for the service.
    fn name(&self) -> &'static str;

    /// Start the service and return once it has finished initializing.
    async fn start(&self, ctx: ServiceContext) -> Result<(), ServiceError>;

    /// Request a graceful shutdown of the service.
    async fn stop(&self) -> Result<(), ServiceError>;

    /// Current status of the service.
    fn status(&self) -> ServiceStatus;
}
