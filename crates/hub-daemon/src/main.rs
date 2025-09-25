use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use async_trait::async_trait;
use lokan_automation::{create_echo_rule, RuleEngine};
use lokan_core::{
    LokanConfig, Service, ServiceContext, ServiceError, ServiceManager, ServiceStatus,
};
use lokan_device::{DeviceDescriptor, DeviceDriver, DeviceError, DeviceRegistry, DeviceState};
use lokan_event::{Event, EventBus};
use lokan_network::{ConnectionParams, ConnectivitySupervisor, MqttConnector};
use serde_json::json;
use tokio::{signal, sync::Mutex as AsyncMutex, task::JoinHandle, time};
use tracing::{info, warn};

const EVENT_BUS_KEY: &str = "event_bus";
const DEVICE_REGISTRY_KEY: &str = "device_registry";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = LokanConfig::default();
    let event_bus = EventBus::new(1024);
    let device_registry = DeviceRegistry::new();

    let manager = ServiceManager::new(config)
        .with_extension(EVENT_BUS_KEY, Arc::new(event_bus.clone()))
        .with_extension(DEVICE_REGISTRY_KEY, Arc::new(device_registry.clone()));

    let mut manager = manager;
    manager.register_service(Arc::new(AutomationService::new()));
    manager.register_service(Arc::new(DeviceMonitorService::new()));

    manager.start_all().await?;
    info!("Lokan Home Hub runtime started");

    signal::ctrl_c().await?;
    info!("shutdown signal received");

    manager.stop_all().await;

    Ok(())
}

fn init_tracing() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .compact()
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

struct AutomationService {
    engine: AsyncMutex<Option<Arc<RuleEngine>>>,
    handle: AsyncMutex<Option<JoinHandle<()>>>,
    status: Mutex<ServiceStatus>,
}

impl AutomationService {
    fn new() -> Self {
        Self {
            engine: AsyncMutex::new(None),
            handle: AsyncMutex::new(None),
            status: Mutex::new(ServiceStatus::Stopped),
        }
    }
}

#[async_trait]
impl Service for AutomationService {
    fn name(&self) -> &'static str {
        "automation-service"
    }

    async fn start(&self, ctx: ServiceContext) -> Result<(), ServiceError> {
        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Starting;
        }

        let event_bus = ctx
            .get_extension::<EventBus>(EVENT_BUS_KEY)
            .ok_or_else(|| ServiceError::Initialization("event bus not available".into()))?;

        let engine = Arc::new(RuleEngine::new(event_bus.as_ref().clone()));
        engine
            .register_rule(create_echo_rule("sensors.temperature"))
            .await
            .map_err(|err| ServiceError::Initialization(err.to_string()))?;

        let runner = Arc::clone(&engine);
        let handle = tokio::spawn(async move {
            if let Err(err) = runner.run().await {
                warn!(error = %err, "rule engine stopped");
            }
        });

        {
            let mut engine_slot = self.engine.lock().await;
            *engine_slot = Some(engine);
        }

        {
            let mut handle_slot = self.handle.lock().await;
            *handle_slot = Some(handle);
        }

        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Running;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Stopping;
        }

        if let Some(handle) = self.handle.lock().await.take() {
            handle.abort();
        }

        {
            let mut engine_slot = self.engine.lock().await;
            *engine_slot = None;
        }

        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Stopped;
        }

        Ok(())
    }

    fn status(&self) -> ServiceStatus {
        *self.status.lock().unwrap()
    }
}

struct DeviceMonitorService {
    driver: Arc<MockTemperatureDriver>,
    handle: AsyncMutex<Option<JoinHandle<()>>>,
    descriptor: AsyncMutex<Option<DeviceDescriptor>>,
    registry: AsyncMutex<Option<Arc<DeviceRegistry>>>,
    supervisor: AsyncMutex<Option<ConnectivitySupervisor<MqttConnector>>>,
    status: Mutex<ServiceStatus>,
}

impl DeviceMonitorService {
    fn new() -> Self {
        Self {
            driver: Arc::new(MockTemperatureDriver::default()),
            handle: AsyncMutex::new(None),
            descriptor: AsyncMutex::new(None),
            registry: AsyncMutex::new(None),
            supervisor: AsyncMutex::new(None),
            status: Mutex::new(ServiceStatus::Stopped),
        }
    }
}

#[async_trait]
impl Service for DeviceMonitorService {
    fn name(&self) -> &'static str {
        "device-monitor"
    }

    async fn start(&self, ctx: ServiceContext) -> Result<(), ServiceError> {
        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Starting;
        }

        let event_bus = ctx
            .get_extension::<EventBus>(EVENT_BUS_KEY)
            .ok_or_else(|| ServiceError::Initialization("event bus not available".into()))?;

        let registry = ctx
            .get_extension::<DeviceRegistry>(DEVICE_REGISTRY_KEY)
            .ok_or_else(|| ServiceError::Initialization("device registry not available".into()))?;

        let descriptor = DeviceDescriptor {
            id: "virtual.temp.sensor".into(),
            manufacturer: "Lokan Labs".into(),
            product: "Virtual Temperature Sensor".into(),
            capabilities: vec!["temperature".into()],
        };

        registry
            .register_device(descriptor.clone(), self.driver.as_ref())
            .await
            .map_err(|err| ServiceError::Initialization(err.to_string()))?;

        let connection = ConnectivitySupervisor::new(
            MqttConnector,
            ConnectionParams {
                endpoint: "mqtt://localhost:1883".into(),
                username: None,
                password: None,
                keep_alive_secs: Some(30),
            },
        );
        connection.ensure_connected().await;

        {
            let mut supervisor_slot = self.supervisor.lock().await;
            *supervisor_slot = Some(connection);
        }

        {
            let mut registry_slot = self.registry.lock().await;
            *registry_slot = Some(registry.clone());
        }

        {
            let mut descriptor_slot = self.descriptor.lock().await;
            *descriptor_slot = Some(descriptor.clone());
        }

        let driver = self.driver.clone();
        let registry_clone = registry.clone();
        let descriptor_clone = descriptor.clone();
        let event_bus_clone = event_bus.as_ref().clone();

        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                match driver.poll(&descriptor_clone).await {
                    Ok(state) => {
                        let _ = registry_clone
                            .update_state(&descriptor_clone.id, state.clone())
                            .await;
                        let payload = json!({
                            "device_id": descriptor_clone.id,
                            "temperature_c": state.properties["temperature_c"].clone(),
                        });
                        event_bus_clone.publish(Event::new("sensors.temperature", payload));
                    }
                    Err(err) => {
                        warn!(device_id = %descriptor_clone.id, error = %err, "failed to poll device");
                    }
                }
            }
        });

        {
            let mut handle_slot = self.handle.lock().await;
            *handle_slot = Some(handle);
        }

        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Running;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Stopping;
        }

        if let Some(handle) = self.handle.lock().await.take() {
            handle.abort();
        }

        if let Some(connection) = self.supervisor.lock().await.take() {
            connection.shutdown().await;
        }

        if let Some(registry) = self.registry.lock().await.take() {
            if let Some(descriptor) = self.descriptor.lock().await.take() {
                registry
                    .unregister_device(&descriptor.id, self.driver.as_ref())
                    .await
                    .map_err(|err| ServiceError::Shutdown(err.to_string()))?;
            }
        }

        {
            let mut status = self.status.lock().unwrap();
            *status = ServiceStatus::Stopped;
        }

        Ok(())
    }

    fn status(&self) -> ServiceStatus {
        *self.status.lock().unwrap()
    }
}

#[derive(Default)]
struct MockTemperatureDriver {
    temperature: AsyncMutex<f64>,
}

#[async_trait]
impl DeviceDriver for MockTemperatureDriver {
    async fn initialize(&self, descriptor: &DeviceDescriptor) -> Result<(), DeviceError> {
        info!(device_id = %descriptor.id, "mock driver initialized");
        Ok(())
    }

    async fn poll(&self, _descriptor: &DeviceDescriptor) -> Result<DeviceState, DeviceError> {
        let mut value = self.temperature.lock().await;
        *value += 0.5;
        if *value > 40.0 {
            *value = 20.0;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));

        let state = DeviceState {
            online: true,
            last_seen_epoch_ms: now.as_millis() as u64,
            properties: json!({
                "temperature_c": (*value),
            }),
        };

        Ok(state)
    }

    async fn shutdown(&self, descriptor: &DeviceDescriptor) -> Result<(), DeviceError> {
        info!(device_id = %descriptor.id, "mock driver shutdown");
        Ok(())
    }
}
