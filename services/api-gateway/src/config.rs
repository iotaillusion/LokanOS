use std::net::SocketAddr;
use std::path::PathBuf;

use common_config::{MsgBusConfig, ServiceConfig};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ApiGatewayConfig {
    pub bind_address: String,
    pub port: u16,
    pub announce_mdns: bool,
    pub mdns_service: String,
    #[serde(flatten)]
    pub bus: MsgBusConfig,
    pub tls: TlsConfig,
    pub rbac_policy_path: PathBuf,
    pub audit: AuditConfig,
    pub device_registry_url: String,
    pub rate_limit: RateLimitSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub client_ca_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    pub endpoint: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimitSettings {
    pub requests_per_minute: u32,
    pub burst: u32,
}

impl Default for ApiGatewayConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8443,
            announce_mdns: true,
            mdns_service: "_lokan._tcp".to_string(),
            bus: MsgBusConfig::default(),
            tls: TlsConfig::default(),
            rbac_policy_path: PathBuf::from("configs/rbac.yaml"),
            audit: AuditConfig::default(),
            device_registry_url: "http://127.0.0.1:8001".to_string(),
            rate_limit: RateLimitSettings::default(),
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_path: PathBuf::from(
                "security/pki/dev/out/services/api-gateway/api-gateway.cert.pem",
            ),
            key_path: PathBuf::from(
                "security/pki/dev/out/services/api-gateway/api-gateway.key.pem",
            ),
            client_ca_path: PathBuf::from("security/pki/dev/out/ca/lokan-dev-root-ca.cert.pem"),
        }
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8008/v1/events".to_string(),
            enabled: true,
        }
    }
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            requests_per_minute: 120,
            burst: 40,
        }
    }
}

impl ApiGatewayConfig {
    pub fn socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.bind_address, self.port).parse()
    }
}

impl ServiceConfig for ApiGatewayConfig {
    const PREFIX: &'static str = "API_GATEWAY_";

    fn apply_environment_overrides(&mut self, prefix: &str) {
        self.bus.apply_environment_overrides(prefix);
    }
}
