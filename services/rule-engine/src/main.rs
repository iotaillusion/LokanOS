use std::net::SocketAddr;

use axum::Router;
use common_config::service_port;
use common_obs::health_router;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "rule-engine";
const PORT_ENV: &str = "RULE_ENGINE_PORT";
const DEFAULT_PORT: u16 = 8002;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, service = SERVICE_NAME, "starting service");

    let app = Router::new().merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
