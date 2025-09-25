//! Shared configuration helpers for LokanOS services.

use std::env;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use thiserror::Error;

/// Attempt to load variables from a local `.env` file while keeping real environment
/// overrides intact.
fn load_dotenv() {
    match dotenvy::dotenv() {
        Ok(path) => tracing::debug!(?path, "loaded dotenv file"),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::trace!("no .env file found")
        }
        Err(error) => tracing::warn!(%error, "failed to parse .env file"),
    }
}

/// Errors that may occur while loading service configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Wrapper around deserialization failures.
    #[error("failed to deserialize configuration from environment: {0}")]
    Deserialize(#[from] envy::Error),
}

/// Trait implemented by strongly typed configuration structs for services.
pub trait ServiceConfig: DeserializeOwned + Default {
    /// Environment variable prefix used to load the configuration.
    const PREFIX: &'static str;

    /// Allow implementors to tweak values after environment loading.
    fn apply_environment_overrides(&mut self, _prefix: &str) {}
}

/// Load a strongly typed configuration struct for a service using its declared prefix.
pub fn load<T>() -> Result<T, ConfigError>
where
    T: ServiceConfig,
{
    let mut config = load_with_prefix::<T>(T::PREFIX)?;
    config.apply_environment_overrides(T::PREFIX);
    Ok(config)
}

/// Load a configuration struct using the provided environment prefix.
pub fn load_with_prefix<T>(prefix: &str) -> Result<T, ConfigError>
where
    T: DeserializeOwned + Default,
{
    load_dotenv();

    if prefix.is_empty() {
        envy::from_env::<T>().map_err(ConfigError::from)
    } else {
        let vars = env::vars().filter_map(|(key, value)| {
            key.strip_prefix(prefix)
                .map(|stripped| (stripped.to_string(), value))
        });
        envy::from_iter(vars).map_err(ConfigError::from)
    }
}

/// Common configuration shared across services for connecting to the message bus.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MsgBusConfig {
    /// Connection URL for the message bus backend.
    pub url: String,
    /// Timeout (in milliseconds) for request/response flows.
    pub request_timeout_ms: u64,
}

impl Default for MsgBusConfig {
    fn default() -> Self {
        Self {
            url: "nats://127.0.0.1:4222".to_string(),
            request_timeout_ms: 2_000,
        }
    }
}

impl MsgBusConfig {
    /// Timeout for bus requests as a [`Duration`].
    pub fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }

    /// Overlay environment-driven overrides onto the existing configuration.
    pub fn apply_environment_overrides(&mut self, prefix: &str) {
        if let Some(url) = env_value(prefix, &["BUS__URL", "BUS_URL"]) {
            self.url = url;
        }

        if let Some(timeout) = env_value(
            prefix,
            &["BUS__REQUEST_TIMEOUT_MS", "BUS_REQUEST_TIMEOUT_MS"],
        ) {
            match timeout.parse::<u64>() {
                Ok(value) => self.request_timeout_ms = value,
                Err(error) => tracing::warn!(%timeout, %error, "invalid bus timeout override"),
            }
        }
    }
}

fn env_value(prefix: &str, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|suffix| {
            if prefix.is_empty() {
                env::var(suffix).ok()
            } else {
                let key = format!("{prefix}{suffix}");
                env::var(key).ok()
            }
        })
        .next()
}

/// Resolve the port for a service from an environment variable.
///
/// Falls back to the provided default when the variable is missing or cannot be
/// parsed into a `u16`.
pub fn service_port(var: &str, default: u16) -> u16 {
    match env::var(var) {
        Ok(value) => value
            .parse::<u16>()
            .inspect_err(|error| {
                tracing::warn!(%var, %value, %error, "invalid port override, using default");
            })
            .unwrap_or(default),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::{load, MsgBusConfig, ServiceConfig};

    #[derive(Debug, Clone, serde::Deserialize, PartialEq)]
    #[serde(default)]
    struct TestConfig {
        value: String,
        number: u16,
        bus: MsgBusConfig,
    }

    impl Default for TestConfig {
        fn default() -> Self {
            Self {
                value: "default".to_string(),
                number: 10,
                bus: MsgBusConfig::default(),
            }
        }
    }

    #[test]
    fn env_overrides_dotenv_and_defaults() {
        std::env::remove_var("TEST_VALUE");
        std::env::remove_var("TEST_NUMBER");
        std::env::remove_var("TEST_BUS__URL");

        // Simulate values that would normally come from the `.env` file.
        std::env::set_var("TEST_VALUE", "from_dotenv");
        std::env::set_var("TEST_BUS__URL", "nats://embedded");
        std::env::set_var("TEST_BUS_URL", "nats://embedded");

        // Explicit environment variables should take precedence.
        std::env::set_var("TEST_VALUE", "from_env");
        std::env::set_var("TEST_NUMBER", "42");

        let config: TestConfig = load().expect("load config");
        assert_eq!(config.value, "from_env");
        assert_eq!(config.number, 42);
        assert_eq!(config.bus.url, "nats://embedded");
    }

    impl ServiceConfig for TestConfig {
        const PREFIX: &'static str = "TEST_";

        fn apply_environment_overrides(&mut self, prefix: &str) {
            self.bus.apply_environment_overrides(prefix);
        }
    }
}
