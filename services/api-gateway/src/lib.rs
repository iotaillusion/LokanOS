pub mod audit;
pub mod commissioning;
pub mod config;
pub mod device_registry;
pub mod error;
pub mod rate_limit;
pub mod rbac;

use std::sync::Arc;

use audit::{AuditClient, AuditEvent};
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use commissioning::{ble_handshake, submit_csr, verify_credentials};
use common_msgbus::MessageBus;
use device_registry::DeviceRegistryClient;
use error::ApiError;
use rate_limit::RateLimiter;
use rbac::{PolicyError, RbacPolicy, Role};
use serde_json::json;

pub const SERVICE_NAME: &str = "api-gateway";
pub const ROLE_HEADER: &str = "x-lokan-role";
pub const SUBJECT_HEADER: &str = "x-lokan-subject";

#[derive(Clone)]
pub struct AppState {
    pub policy: Arc<RbacPolicy>,
    pub audit: AuditClient,
    pub rate_limiter: RateLimiter,
    pub device_client: DeviceRegistryClient,
    pub bus: Arc<dyn MessageBus>,
}

#[derive(Clone, Debug)]
pub struct UserContext {
    pub subject: String,
    pub role: Role,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/info", get(info))
        .route(
            "/v1/devices",
            get(list_devices).post(devices_not_implemented),
        )
        .route("/v1/commissioning/ble/handshake", post(ble_handshake))
        .route("/v1/commissioning/csr", post(submit_csr))
        .route("/v1/commissioning/verify", post(verify_credentials))
        .layer(from_fn_with_state(state.clone(), rate_limit_guard))
        .layer(from_fn_with_state(state.clone(), rbac_guard))
        .layer(Extension(state.clone()))
        .with_state(state)
}

pub async fn rate_limit_guard(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    if let Err(err) = state.rate_limiter.check().await {
        let role = extract_role(req.headers());
        let subject = extract_subject(req.headers());
        let path = req.uri().path().to_string();
        let event = AuditEvent::new(
            subject,
            role.as_str().to_string(),
            "rate_limit.check".to_string(),
            path,
            "throttle".to_string(),
        )
        .with_detail(json!({ "reason": "rate limit exceeded" }));
        state.audit.record(event).await;
        return Err(err);
    }

    Ok(next.run(req).await)
}

pub async fn rbac_guard(
    State(state): State<Arc<AppState>>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let role = extract_role(req.headers());
    let subject = extract_subject(req.headers());
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    let decision = state.policy.authorize(role, &method, &path);
    let action = decision
        .audit_action
        .clone()
        .unwrap_or_else(|| format!("{} {}", method, path));
    let mut event = AuditEvent::new(
        subject.clone(),
        role.as_str().to_string(),
        action,
        path.clone(),
        "deny".to_string(),
    )
    .with_detail(json!({ "method": method.as_str() }));

    if !decision.allowed {
        state.audit.record(event.clone()).await;
        return Err(ApiError::Forbidden {
            reason: format!("role {} is not permitted to access {}", role.as_str(), path),
        });
    }

    req.extensions_mut().insert(UserContext {
        subject: subject.clone(),
        role,
    });

    let response = next.run(req).await;

    event = event.with_outcome("allow");
    state.audit.record(event).await;

    Ok(response)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": SERVICE_NAME }))
}

async fn info() -> Json<serde_json::Value> {
    Json(json!({
        "service": SERVICE_NAME,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn list_devices(
    Extension(state): Extension<Arc<AppState>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let payload = state.device_client.list_devices().await?;
    let devices = payload.get("devices").cloned().unwrap_or(payload);
    Ok(Json(json!({
        "requested_by": {
            "subject": user.subject,
            "role": user.role.as_str(),
        },
        "devices": devices,
    })))
}

async fn devices_not_implemented() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "code": "not_implemented",
                "message": "device provisioning is not yet implemented",
            }
        })),
    )
}

pub fn load_policy(config: &config::ApiGatewayConfig) -> Result<RbacPolicy, PolicyError> {
    RbacPolicy::from_path(&config.rbac_policy_path)
}

pub fn extract_role(headers: &HeaderMap) -> Role {
    headers
        .get(ROLE_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(Role::Guest)
}

pub fn extract_subject(headers: &HeaderMap) -> String {
    headers
        .get(SUBJECT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "anonymous".to_string())
}
