use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router};
use common_config::service_port;
use common_obs::health_router;
use serde::Serialize;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "device-registry";
const PORT_ENV: &str = "DEVICE_REGISTRY_PORT";
const DEFAULT_PORT: u16 = 8001;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, service = SERVICE_NAME, "starting service");

    let app = Router::new()
        .route("/v1/devices", get(list_devices))
        .merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[derive(Debug, Serialize)]
struct DeviceRecord {
    id: String,
    name: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct DevicesResponse {
    devices: Vec<DeviceRecord>,
}

async fn list_devices() -> Json<DevicesResponse> {
    Json(DevicesResponse {
        devices: vec![
            DeviceRecord {
                id: "device-1".to_string(),
                name: "Test Thermostat".to_string(),
                status: "online".to_string(),
            },
            DeviceRecord {
                id: "device-2".to_string(),
                name: "Garage Door".to_string(),
                status: "offline".to_string(),
            },
        ],
    })
}
