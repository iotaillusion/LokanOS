use std::net::SocketAddr;

use axum::Router;
use common_config::{load, MsgBusConfig, ServiceConfig};
use common_mdns::announce;
use common_msgbus::{NatsBus, NatsConfig};
use common_obs::health_router;
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "radio-coord";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = load::<RadioCoordConfig>()?;
    let addr = config.socket_addr()?;
    tracing::info!(%addr, service = SERVICE_NAME, "starting service");

    let bus_config = NatsConfig {
        url: config.bus.url.clone(),
        request_timeout: config.bus.request_timeout(),
    };
    let _bus = NatsBus::connect(bus_config).await?;

    let _mdns = if config.announce_mdns {
        Some(announce(&config.mdns_service, config.port).await?)
    } else {
        tracing::info!(service = SERVICE_NAME, "mDNS announcement disabled");
        None
    };

    let app = Router::new().merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct RadioCoordConfig {
    pub bind_address: String,
    pub port: u16,
    pub announce_mdns: bool,
    pub mdns_service: String,
    #[serde(flatten)]
    pub bus: MsgBusConfig,
}

impl Default for RadioCoordConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8009,
            announce_mdns: true,
            mdns_service: "_lokan._tcp".to_string(),
            bus: MsgBusConfig::default(),
        }
    }
}

impl ServiceConfig for RadioCoordConfig {
    const PREFIX: &'static str = "RADIO_COORD_";

    fn apply_environment_overrides(&mut self, prefix: &str) {
        self.bus.apply_environment_overrides(prefix);
    }
}

impl RadioCoordConfig {
    fn socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.bind_address, self.port).parse()
    }
}
