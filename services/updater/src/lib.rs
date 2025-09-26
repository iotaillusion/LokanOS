use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, http_request_observe, ObsInit, ObsInitError, PROMETHEUS_CONTENT_TYPE,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

pub mod bundle;
mod core;
mod health;
mod state;
mod store;

pub use crate::core::{UpdaterCore, UpdaterError};
pub use crate::health::{HealthCheckError, HealthClient, StubHealthClient};
pub use crate::state::{Slot, SlotState, UpdaterState};
pub use crate::store::{FileStateStore, MemoryStateStore, StateStore};

use crate::bundle::FilesystemBundleVerifier;
use crate::health::HttpHealthClient;
use crate::state::{CommitError, StageError};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_SHA: &str = match option_env!("BUILD_SHA") {
    Some(value) => value,
    None => "dev",
};
const BUILD_TIME: &str = match option_env!("BUILD_TIME") {
    Some(value) => value,
    None => "1970-01-01T00:00:00Z",
};

pub const SERVICE_NAME: &str = "updater";
pub const PORT_ENV: &str = "UPDATER_PORT";
pub const DEFAULT_PORT: u16 = 8006;
const DEFAULT_STATE_PATH: &str = "data/updater/state.json";
const HEALTH_DEADLINE_ENV: &str = "UPDATER_HEALTH_DEADLINE_SECS";
const HEALTH_ENDPOINTS_ENV: &str = "UPDATER_HEALTH_ENDPOINTS";
const HEALTH_QUORUM_ENV: &str = "UPDATER_HEALTH_QUORUM";
const DEFAULT_HEALTH_DEADLINE: Duration = Duration::from_secs(30);
const OTA_PUBLIC_KEY_ENV: &str = "UPDATER_OTA_PUBLIC_KEY";
const DEFAULT_OTA_PUBLIC_KEY_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../security/pki/dev/ota/ota_signing_public.pem"
);

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = BUILD_SHA,
        build_time = BUILD_TIME,
        listen_addr = %addr,
        "starting service",
    );

    serve(addr).await
}

pub async fn serve(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let app = build_router()
        .await
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

pub async fn build_router() -> Result<Router, UpdaterError> {
    let core = default_core().await?;
    Ok(router_with_core(core))
}

fn router_with_core(core: UpdaterCore) -> Router {
    let app_state = AppState { core };

    let api = Router::new()
        .route("/v1/update/stage", post(stage))
        .route("/v1/update/commit", post(commit))
        .route("/v1/update/rollback", post(rollback))
        .route("/v1/update/status", get(status))
        .with_state(app_state);

    Router::new()
        .route("/metrics", get(metrics))
        .merge(api)
        .merge(health_routes())
        .layer(from_fn(track_http_metrics))
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

async fn track_http_metrics(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or(path);

    let start = Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    http_request_observe!(route.as_str(), status.as_str(), latency);

    response
}

pub fn init_for_tests() -> Result<(), ObsInitError> {
    ObsInit::init(SERVICE_NAME)
}

#[derive(Clone)]
struct AppState {
    core: UpdaterCore,
}

#[derive(Debug, Deserialize)]
struct StageRequest {
    artifact: String,
}

#[derive(Debug, Serialize)]
struct SlotResponse {
    slot: Slot,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct HealthResponseBody {
    status: &'static str,
}

async fn stage(State(state): State<AppState>, Json(payload): Json<StageRequest>) -> Response {
    match state.core.stage(payload.artifact).await {
        Ok(slot) => (StatusCode::ACCEPTED, Json(SlotResponse { slot })).into_response(),
        Err(err) => error_response(err),
    }
}

async fn commit(State(state): State<AppState>) -> Response {
    match state.core.commit_on_health().await {
        Ok(slot) => (StatusCode::OK, Json(SlotResponse { slot })).into_response(),
        Err(err) => error_response(err),
    }
}

async fn rollback(State(state): State<AppState>) -> Response {
    match state.core.rollback().await {
        Ok(slot) => (StatusCode::OK, Json(SlotResponse { slot })).into_response(),
        Err(err) => error_response(err),
    }
}

async fn status(State(state): State<AppState>) -> Response {
    let snapshot = state.core.state().await;
    Json(snapshot).into_response()
}

async fn health_contract() -> Response {
    Json(HealthResponseBody { status: "ok" }).into_response()
}

#[derive(Debug, Serialize)]
struct InfoResponseBody {
    service: &'static str,
    version: &'static str,
}

fn health_routes() -> Router {
    Router::new()
        .route("/health", get(health_contract))
        .route("/v1/health", get(health_contract))
        .route("/info", get(info_contract))
        .route("/v1/info", get(info_contract))
}

async fn info_contract() -> Response {
    Json(InfoResponseBody {
        service: SERVICE_NAME,
        version: VERSION,
    })
    .into_response()
}

fn error_response(err: UpdaterError) -> Response {
    let status = match &err {
        UpdaterError::Stage(StageError::SlotBooting) => StatusCode::CONFLICT,
        UpdaterError::Stage(_) => StatusCode::BAD_REQUEST,
        UpdaterError::Commit(CommitError::NothingStaged) => StatusCode::BAD_REQUEST,
        UpdaterError::Commit(CommitError::InvalidStageState) => StatusCode::CONFLICT,
        UpdaterError::Rollback(_) => StatusCode::CONFLICT,
        UpdaterError::HealthQuorumFailed => StatusCode::SERVICE_UNAVAILABLE,
        UpdaterError::Store(_) | UpdaterError::Health(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        Json(ErrorResponse {
            error: err.to_string(),
        }),
    )
        .into_response()
}

async fn default_core() -> Result<UpdaterCore, UpdaterError> {
    let store = Arc::new(FileStateStore::new(DEFAULT_STATE_PATH));
    let endpoints = health_endpoints_from_env();
    let quorum = health_quorum_from_env(endpoints.len());
    let health_client = Arc::new(HttpHealthClient::default());
    let deadline = health_deadline_from_env();

    let public_key_path = std::env::var(OTA_PUBLIC_KEY_ENV)
        .unwrap_or_else(|_| DEFAULT_OTA_PUBLIC_KEY_PATH.to_string());
    let bundle_verifier = Arc::new(
        FilesystemBundleVerifier::from_public_key_pem(&public_key_path)
            .map_err(|err| StageError::InvalidBundle(err.to_string()))?,
    );

    UpdaterCore::new(
        store,
        health_client,
        endpoints,
        deadline,
        quorum,
        bundle_verifier,
    )
    .await
}

fn health_endpoints_from_env() -> Vec<String> {
    std::env::var(HEALTH_ENDPOINTS_ENV)
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|item| {
                    let trimmed = item.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn health_deadline_from_env() -> Duration {
    std::env::var(HEALTH_DEADLINE_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_HEALTH_DEADLINE)
}

fn health_quorum_from_env(default: usize) -> usize {
    std::env::var(HEALTH_QUORUM_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
pub fn router_for_tests(core: UpdaterCore) -> Router {
    router_with_core(core)
}
