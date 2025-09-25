use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("protocol operation failed: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionParams {
    pub endpoint: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub keep_alive_secs: Option<u64>,
}

#[async_trait]
pub trait ProtocolConnector: Send + Sync {
    fn name(&self) -> &'static str;
    async fn connect(&self, params: &ConnectionParams) -> Result<(), NetworkError>;
    async fn disconnect(&self);
}

pub struct MqttConnector;

#[async_trait]
impl ProtocolConnector for MqttConnector {
    fn name(&self) -> &'static str {
        "mqtt"
    }

    async fn connect(&self, params: &ConnectionParams) -> Result<(), NetworkError> {
        info!(endpoint = %params.endpoint, "connecting to MQTT broker");
        sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn disconnect(&self) {
        info!("disconnecting from MQTT broker");
    }
}

pub struct MatterConnector;

#[async_trait]
impl ProtocolConnector for MatterConnector {
    fn name(&self) -> &'static str {
        "matter"
    }

    async fn connect(&self, params: &ConnectionParams) -> Result<(), NetworkError> {
        info!(endpoint = %params.endpoint, "bootstrapping Matter fabric");
        sleep(Duration::from_millis(150)).await;
        Ok(())
    }

    async fn disconnect(&self) {
        info!("stopping Matter stack");
    }
}

pub struct ZigbeeConnector;

#[async_trait]
impl ProtocolConnector for ZigbeeConnector {
    fn name(&self) -> &'static str {
        "zigbee"
    }

    async fn connect(&self, params: &ConnectionParams) -> Result<(), NetworkError> {
        info!(endpoint = %params.endpoint, "initializing Zigbee coordinator");
        sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn disconnect(&self) {
        info!("shutting down Zigbee network");
    }
}

/// Supervises protocol connectors and handles reconnection logic.
pub struct ConnectivitySupervisor<C: ProtocolConnector> {
    connector: C,
    params: ConnectionParams,
}

impl<C: ProtocolConnector> ConnectivitySupervisor<C> {
    pub fn new(connector: C, params: ConnectionParams) -> Self {
        Self { connector, params }
    }

    pub async fn ensure_connected(&self) {
        if let Err(err) = self.connector.connect(&self.params).await {
            warn!(connector = self.connector.name(), error = %err, "failed to connect");
        }
    }

    pub async fn shutdown(&self) {
        self.connector.disconnect().await;
    }
}
