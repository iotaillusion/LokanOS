use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Global configuration for the Lokan Home Hub runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LokanConfig {
    /// Location on disk where persistent state is stored.
    pub data_dir: String,
    /// Network specific configuration knobs.
    pub network: NetworkConfig,
    /// Automation and rule engine configuration.
    pub automation: AutomationConfig,
    /// Telemetry and tracing configuration.
    pub telemetry: TelemetryConfig,
}

impl Default for LokanConfig {
    fn default() -> Self {
        Self {
            data_dir: "/var/lib/lokan".into(),
            network: NetworkConfig::default(),
            automation: AutomationConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

/// Error type for configuration related failures.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
}

impl LokanConfig {
    /// Load the configuration from a TOML file on disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Persist the configuration to a TOML file on disk.
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let serialized = toml::to_string_pretty(self)
            .expect("serialization to string should not fail for valid config");
        fs::write(path, serialized)
    }
}

/// Networking configuration shared across protocols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Hostname used to advertise the hub on the network.
    pub hostname: String,
    /// Optional MQTT broker endpoint.
    pub mqtt_broker: Option<String>,
    /// Whether the built-in Matter stack should be enabled.
    pub enable_matter: bool,
    /// Whether the built-in Zigbee stack should be enabled.
    pub enable_zigbee: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            hostname: "lokan-hub".into(),
            mqtt_broker: None,
            enable_matter: true,
            enable_zigbee: false,
        }
    }
}

/// Automation specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    /// Maximum number of rules that can be registered at runtime.
    pub max_rules: usize,
    /// Whether rules are enabled globally.
    pub enabled: bool,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            max_rules: 1024,
            enabled: true,
        }
    }
}

/// Telemetry configuration for the hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether trace logs should be emitted.
    pub enable_tracing: bool,
    /// Optional OTLP collector endpoint for streaming telemetry.
    pub otlp_endpoint: Option<String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enable_tracing: true,
            otlp_endpoint: None,
        }
    }
}
