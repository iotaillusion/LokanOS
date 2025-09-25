use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{service::ServiceContext, LokanConfig, Service, ServiceError};

/// Central orchestrator for services that make up the Lokan Home Hub runtime.
pub struct ServiceManager {
    config: Arc<LokanConfig>,
    extensions: HashMap<String, Arc<dyn std::any::Any + Send + Sync>>,
    services: Vec<Arc<dyn Service>>,
    started: Arc<RwLock<bool>>,
}

impl ServiceManager {
    pub fn new(config: LokanConfig) -> Self {
        Self {
            config: Arc::new(config),
            extensions: HashMap::new(),
            services: Vec::new(),
            started: Arc::new(RwLock::new(false)),
        }
    }

    /// Registers an extension that should be visible to all services.
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    /// Register a service instance with the runtime.
    pub fn register_service(&mut self, service: Arc<dyn Service>) {
        self.services.push(service);
    }

    /// Start all registered services sequentially.
    pub async fn start_all(&self) -> Result<(), ServiceError> {
        {
            let mut started = self.started.write().await;
            if *started {
                return Ok(());
            }
            *started = true;
        }

        let mut ctx = ServiceContext::new(self.config.clone());
        for (key, value) in &self.extensions {
            ctx = ctx.with_extension(key.clone(), value.clone());
        }

        for service in &self.services {
            info!(service = service.name(), "starting service");
            if let Err(err) = service.start(ctx.clone()).await {
                warn!(service = service.name(), error = %err, "service failed to start");
                return Err(err);
            }
        }

        Ok(())
    }

    /// Stop all services in reverse order.
    pub async fn stop_all(&self) {
        for service in self.services.iter().rev() {
            info!(service = service.name(), "stopping service");
            if let Err(err) = service.stop().await {
                warn!(service = service.name(), error = %err, "service failed to stop cleanly");
            }
        }

        let mut started = self.started.write().await;
        *started = false;
    }

    pub fn config(&self) -> Arc<LokanConfig> {
        self.config.clone()
    }
}
