pub mod audit;
pub mod commissioning;
pub mod config;
pub mod device_registry;
pub mod error;
pub mod rate_limit;
pub mod rbac;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audit::{AuditClient, AuditEvent};
use axum::body::Body;
use axum::extract::{connect_info::ConnectInfo, Extension, MatchedPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use commissioning::{ble_handshake, submit_csr, verify_credentials};
use common_msgbus::MessageBus;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, http_requests_total, SpanExt,
    PROMETHEUS_CONTENT_TYPE,
};
use device_registry::DeviceRegistryClient;
use error::ApiError;
use rate_limit::RateLimiter;
use rbac::{PolicyError, RbacPolicy, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;
use tracing::info_span;
use uuid::Uuid;

pub const SERVICE_NAME: &str = "api-gateway";
pub const ROLE_HEADER: &str = "x-lokan-role";
pub const SUBJECT_HEADER: &str = "x-lokan-subject";
const REQUEST_ID_HEADER: &str = "x-request-id";

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
    let protected_routes = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/info", get(info))
        .route("/v1/diag/ping", get(diag_ping))
        .route("/v1/diag/routes", get(diag_routes))
        .route(
            "/v1/devices",
            get(list_devices).post(devices_not_implemented),
        )
        .route("/v1/commissioning/ble/handshake", post(ble_handshake))
        .route("/v1/commissioning/csr", post(submit_csr))
        .route("/v1/commissioning/verify", post(verify_credentials))
        .layer(from_fn_with_state(state.clone(), rate_limit_guard))
        .layer(from_fn_with_state(state.clone(), rbac_guard))
        .layer(Extension(state.clone()));

    Router::new()
        .route("/metrics", get(metrics))
        .merge(protected_routes)
        .layer(from_fn(request_context))
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

async fn request_context(mut req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            let id = Uuid::new_v4().to_string();
            req.headers_mut()
                .insert(REQUEST_ID_HEADER, HeaderValue::from_str(&id).unwrap());
            id
        });

    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| path.clone());

    let span = info_span!(
        "http.request",
        method = %method,
        path = %path,
        remote_addr = remote_addr.as_str(),
        user_agent = user_agent.as_str(),
        request_id = %request_id
    );
    span.with_req(&request_id);

    let start = Instant::now();
    let mut response = {
        let _guard = span.enter();
        tracing::info!(
            event = "request_start",
            method = %method,
            path = %path,
            remote_addr = remote_addr.as_str(),
            user_agent = user_agent.as_str()
        );
        next.run(req).await
    };

    let status = response.status();
    let latency = start.elapsed().as_secs_f64();
    {
        let _guard = span.enter();
        tracing::info!(
            event = "request_end",
            method = %method,
            path = %path,
            status = status.as_u16(),
            latency_ms = latency * 1000.0,
            remote_addr = remote_addr.as_str(),
            user_agent = user_agent.as_str()
        );
    }

    let status_label = status.as_u16().to_string();
    http_requests_total().inc(&[SERVICE_NAME, route.as_str(), status_label.as_str()], 1);
    handler_latency_seconds().observe(&[SERVICE_NAME, route.as_str()], latency);

    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&request_id).unwrap(),
    );

    response
}

#[derive(Debug, Deserialize)]
struct PingQuery {
    target: String,
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Debug, Serialize)]
struct PingResponse {
    target: String,
    method: &'static str,
    success: bool,
    duration_ms: f64,
    resolved: Option<Vec<String>>,
    error: Option<String>,
}

async fn diag_ping(Query(params): Query<PingQuery>) -> Result<Json<PingResponse>, ApiError> {
    let target = params.target.trim();
    if target.is_empty() {
        return Err(ApiError::Validation {
            message: "target query parameter is required".to_string(),
        });
    }

    let port = params.port;
    let address = normalize_target(target, port);

    let started = Instant::now();
    let attempt = timeout(Duration::from_secs(2), tcp_probe(address.clone())).await;

    let (resolved, success, error) = match attempt {
        Ok(Ok((resolved, true))) => (resolved, true, None),
        Ok(Ok((resolved, false))) => (
            resolved,
            false,
            Some("unable to establish TCP connection".to_string()),
        ),
        Ok(Err(err)) => (Vec::new(), false, Some(err.to_string())),
        Err(_) => (Vec::new(), false, Some("probe timed out".to_string())),
    };

    let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let resolved = if resolved.is_empty() {
        None
    } else {
        Some(resolved.into_iter().map(|addr| addr.to_string()).collect())
    };

    Ok(Json(PingResponse {
        target: target.to_string(),
        method: "tcp",
        success,
        duration_ms,
        resolved,
        error,
    }))
}

fn normalize_target(target: &str, port_override: Option<u16>) -> String {
    let port = port_override.unwrap_or(80);
    if let Ok(mut socket) = target.parse::<SocketAddr>() {
        if let Some(port) = port_override {
            socket.set_port(port);
        }
        socket.to_string()
    } else if let Ok(ip) = target.parse::<IpAddr>() {
        SocketAddr::new(ip, port).to_string()
    } else if let Some((host, port_str)) = target.rsplit_once(':') {
        if !host.contains(':') && port_str.parse::<u16>().is_ok() {
            if let Some(port) = port_override {
                format!("{host}:{port}")
            } else {
                target.to_string()
            }
        } else {
            format!("{target}:{port}")
        }
    } else {
        format!("{target}:{port}")
    }
}

async fn tcp_probe(address: String) -> std::io::Result<(Vec<SocketAddr>, bool)> {
    let addrs: Vec<SocketAddr> = lookup_host(address.as_str()).await?.collect();
    for addr in &addrs {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok((addrs, true));
        }
    }
    Ok((addrs, false))
}

#[derive(Debug, Serialize)]
struct RoutesResponse {
    guarded: Vec<GuardedRoute>,
    public: Vec<PublicRoute>,
}

#[derive(Debug, Serialize)]
struct GuardedRoute {
    pattern: String,
    methods: Vec<String>,
    allowed_roles: Vec<String>,
    audit_action: Option<String>,
}

#[derive(Debug, Serialize)]
struct PublicRoute {
    path: &'static str,
    methods: &'static [&'static str],
}

async fn diag_routes(State(state): State<Arc<AppState>>) -> Json<RoutesResponse> {
    let mut guarded: Vec<GuardedRoute> = state
        .policy
        .summaries()
        .into_iter()
        .map(|summary| GuardedRoute {
            pattern: summary.pattern,
            methods: summary.methods,
            allowed_roles: summary.allowed_roles,
            audit_action: summary.audit_action,
        })
        .collect();

    guarded.sort_by(|a, b| a.pattern.cmp(&b.pattern));

    let public = vec![PublicRoute {
        path: "/metrics",
        methods: &["GET"],
    }];

    Json(RoutesResponse { guarded, public })
}

async fn metrics() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE),
        )],
        encode_prometheus_metrics(),
    )
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
