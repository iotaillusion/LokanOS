mod audit;
mod config;
mod device_registry;
mod error;
mod rate_limit;
mod rbac;

use std::sync::Arc;

use audit::{AuditClient, AuditEvent};
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use common_config::load;
use common_mdns::announce;
use common_msgbus::{NatsBus, NatsConfig};
use config::{ApiGatewayConfig, TlsConfig};
use device_registry::DeviceRegistryClient;
use error::ApiError;
use rate_limit::RateLimiter;
use rbac::{PolicyError, RbacPolicy, Role};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig as RustlsServerConfig};
use serde_json::json;
use tokio::fs;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "api-gateway";
const ROLE_HEADER: &str = "x-lokan-role";
const SUBJECT_HEADER: &str = "x-lokan-subject";

#[derive(Clone)]
struct AppState {
    policy: Arc<RbacPolicy>,
    audit: AuditClient,
    rate_limiter: RateLimiter,
    device_client: DeviceRegistryClient,
}

#[derive(Clone, Debug)]
struct UserContext {
    subject: String,
    role: Role,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = load::<ApiGatewayConfig>()?;
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

    let policy = Arc::new(load_policy(&config)?);
    let audit = AuditClient::new(config.audit.endpoint.clone(), config.audit.enabled);
    let rate_limiter = RateLimiter::new(&config.rate_limit);
    let device_client = DeviceRegistryClient::new(config.device_registry_url.clone())
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let state = Arc::new(AppState {
        policy,
        audit,
        rate_limiter,
        device_client,
    });

    let router = build_router(state.clone());
    let rustls_config = build_rustls_config(&config.tls).await?;

    axum_server::bind_rustls(addr, rustls_config)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/info", get(info))
        .route(
            "/v1/devices",
            get(list_devices).post(devices_not_implemented),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_guard))
        .layer(from_fn_with_state(state.clone(), rbac_guard))
        .layer(Extension(state))
}

async fn rate_limit_guard(
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

async fn rbac_guard(
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

async fn devices_not_implemented() -> impl axum::response::IntoResponse {
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

fn load_policy(config: &ApiGatewayConfig) -> Result<RbacPolicy, PolicyError> {
    RbacPolicy::from_path(&config.rbac_policy_path)
}

async fn build_rustls_config(
    config: &TlsConfig,
) -> Result<RustlsConfig, Box<dyn std::error::Error>> {
    let certs = load_certs(&config.cert_path).await?;
    let key = load_private_key(&config.key_path).await?;
    let client_store = load_client_ca(&config.client_ca_path).await?;

    let client_verifier = WebPkiClientVerifier::builder(Arc::new(client_store))
        .build()
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
    let server_config = RustlsServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)?;

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}

async fn load_certs(
    path: &std::path::Path,
) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    Ok(certs)
}

async fn load_private_key(
    path: &std::path::Path,
) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<PrivatePkcs8KeyDer<'static>>, _>>()?;
    if let Some(key) = keys.into_iter().next() {
        Ok(PrivateKeyDer::from(key))
    } else {
        Err("no private key found".into())
    }
}

async fn load_client_ca(
    path: &std::path::Path,
) -> Result<RootCertStore, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let mut store = RootCertStore::empty();
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    for cert in certs {
        store.add(cert)?;
    }
    Ok(store)
}

fn extract_role(headers: &axum::http::HeaderMap) -> Role {
    headers
        .get(ROLE_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(Role::Guest)
}

fn extract_subject(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(SUBJECT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "anonymous".to_string())
}
