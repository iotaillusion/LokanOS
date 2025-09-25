use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use common_config::service_port;
use common_obs::health_router;
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "audit-log";
const PORT_ENV: &str = "AUDIT_LOG_PORT";
const DEFAULT_PORT: u16 = 8008;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, service = SERVICE_NAME, "starting service");

    let app = Router::new()
        .route("/v1/events", post(record_event))
        .merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[derive(Debug, Deserialize)]
struct IncomingEvent {
    actor: String,
    role: String,
    action: String,
    resource: String,
    outcome: String,
    #[serde(default)]
    detail: Option<serde_json::Value>,
}

async fn record_event(Json(event): Json<IncomingEvent>) -> StatusCode {
    tracing::info!(
        actor = %event.actor,
        role = %event.role,
        action = %event.action,
        resource = %event.resource,
        outcome = %event.outcome,
        detail = ?event.detail,
        "received audit event"
    );
    StatusCode::ACCEPTED
}
