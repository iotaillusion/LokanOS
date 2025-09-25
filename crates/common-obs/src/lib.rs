//! Observability helpers (logging, health endpoints) for services.

use axum::{extract::State, routing::get, Json, Router};
use serde_json::json;

/// Create a router that exposes a `/health` endpoint.
pub fn health_router(service_name: &'static str) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .with_state(service_name)
}

async fn health_handler(State(service_name): State<&'static str>) -> Json<serde_json::Value> {
    Json(json!({
        "service": service_name,
        "status": "ok",
    }))
}
