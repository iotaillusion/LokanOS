use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// High level device descriptor stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub id: String,
    pub manufacturer: String,
    pub product: String,
    pub capabilities: Vec<String>,
}

/// Runtime state of a device, including its latest readings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceState {
    pub online: bool,
    pub last_seen_epoch_ms: u64,
    pub properties: serde_json::Value,
}

/// Errors originating from device drivers or the registry.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("device with id {0} already exists")]
    AlreadyExists(String),
    #[error("device with id {0} not found")]
    NotFound(String),
    #[error("driver error: {0}")]
    Driver(String),
}

/// Trait implemented by protocol specific device drivers.
#[async_trait]
pub trait DeviceDriver: Send + Sync {
    /// Called when a device is registered with the runtime.
    async fn initialize(&self, descriptor: &DeviceDescriptor) -> Result<(), DeviceError>;

    /// Called to refresh the state of the device.
    async fn poll(&self, descriptor: &DeviceDescriptor) -> Result<DeviceState, DeviceError>;

    /// Called before a device is unregistered.
    async fn shutdown(&self, descriptor: &DeviceDescriptor) -> Result<(), DeviceError>;
}

/// Thread safe in-memory registry for devices managed by the hub.
#[derive(Default, Clone)]
pub struct DeviceRegistry {
    devices: Arc<RwLock<HashMap<String, (DeviceDescriptor, DeviceState)>>>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_device(
        &self,
        descriptor: DeviceDescriptor,
        driver: &dyn DeviceDriver,
    ) -> Result<(), DeviceError> {
        let mut devices = self.devices.write().await;
        if devices.contains_key(&descriptor.id) {
            return Err(DeviceError::AlreadyExists(descriptor.id));
        }
        driver.initialize(&descriptor).await?;
        devices.insert(descriptor.id.clone(), (descriptor, DeviceState::default()));
        Ok(())
    }

    pub async fn unregister_device(
        &self,
        device_id: &str,
        driver: &dyn DeviceDriver,
    ) -> Result<(), DeviceError> {
        let mut devices = self.devices.write().await;
        let (descriptor, _) = devices
            .remove(device_id)
            .ok_or_else(|| DeviceError::NotFound(device_id.into()))?;
        driver.shutdown(&descriptor).await?;
        Ok(())
    }

    pub async fn update_state(
        &self,
        device_id: &str,
        state: DeviceState,
    ) -> Result<(), DeviceError> {
        let mut devices = self.devices.write().await;
        let entry = devices
            .get_mut(device_id)
            .ok_or_else(|| DeviceError::NotFound(device_id.into()))?;
        entry.1 = state;
        Ok(())
    }

    pub async fn poll_device(
        &self,
        device_id: &str,
        driver: &dyn DeviceDriver,
    ) -> Result<DeviceState, DeviceError> {
        let devices = self.devices.read().await;
        let (descriptor, _state) = devices
            .get(device_id)
            .ok_or_else(|| DeviceError::NotFound(device_id.into()))?;
        let updated = driver.poll(descriptor).await?;
        drop(devices);
        self.update_state(device_id, updated.clone()).await?;
        Ok(updated)
    }

    pub async fn list_devices(&self) -> Vec<DeviceDescriptor> {
        let devices = self.devices.read().await;
        devices
            .values()
            .map(|(descriptor, _)| descriptor.clone())
            .collect()
    }

    pub async fn get_state(&self, device_id: &str) -> Option<DeviceState> {
        let devices = self.devices.read().await;
        devices.get(device_id).map(|(_, state)| state.clone())
    }

    pub async fn mark_online(&self, device_id: &str, online: bool) {
        let mut devices = self.devices.write().await;
        if let Some((_descriptor, state)) = devices.get_mut(device_id) {
            state.online = online;
            debug!(device_id, online, "device status changed");
        }
    }

    pub async fn refresh_all(&self, driver: &dyn DeviceDriver) {
        let descriptors = {
            let devices = self.devices.read().await;
            devices
                .values()
                .map(|(descriptor, _)| descriptor.clone())
                .collect::<Vec<_>>()
        };

        for descriptor in descriptors {
            match driver.poll(&descriptor).await {
                Ok(state) => {
                    info!(device_id = %descriptor.id, "device polled successfully");
                    let _ = self.update_state(&descriptor.id, state).await;
                }
                Err(err) => {
                    tracing::warn!(device_id = %descriptor.id, error = %err, "failed to poll device");
                }
            }
        }
    }
}
