use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, health_router, http_requests_total,
    ObsInit, PROMETHEUS_CONTENT_TYPE,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};

use std::time::Instant;

const SERVICE_NAME: &str = "audit-log";
const PORT_ENV: &str = "AUDIT_LOG_PORT";
const DEFAULT_PORT: u16 = 8008;
const DEFAULT_PATH: &str = "audit.log";

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[derive(Clone)]
struct AppState {
    writer: Arc<Mutex<AuditWriter>>,
}

struct AuditWriter {
    path: PathBuf,
    prev_hash: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IncomingEvent {
    actor: String,
    role: String,
    action: String,
    resource: String,
    outcome: String,
    #[serde(default)]
    detail: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditRecord {
    timestamp: DateTime<Utc>,
    prev_hash: String,
    hash: String,
    event: IncomingEvent,
}

#[derive(Debug, thiserror::Error)]
enum AuditError {
    #[error("i/o error: {0}")]
    Io(String),
    #[error("malformed log entry")]
    Malformed,
}

impl From<std::io::Error> for AuditError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl IntoResponse for AuditError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            AuditError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AuditError::Malformed => StatusCode::BAD_REQUEST,
        };
        (
            status,
            Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let log_path = std::env::var("AUDIT_LOG_PATH").unwrap_or_else(|_| DEFAULT_PATH.to_string());

    let writer = AuditWriter::new(PathBuf::from(&log_path)).await?;
    let state = AppState {
        writer: Arc::new(Mutex::new(writer)),
    };

    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = build_sha(),
        build_time = build_time(),
        listen_addr = %addr,
        log_path = %log_path,
        "starting service"
    );

    let app = Router::new()
        .route("/v1/events", post(record_event))
        .route("/v1/events/export", get(export_events))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn record_event(
    State(state): State<AppState>,
    Json(event): Json<IncomingEvent>,
) -> Result<StatusCode, AuditError> {
    let mut writer = state.writer.lock().await;
    writer.append(event).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn export_events(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuditRecord>>, AuditError> {
    let writer = state.writer.lock().await;
    let entries = writer.read_all().await?;
    Ok(Json(entries))
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
        .unwrap_or_else(|| path.clone());

    let start = Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    http_requests_total().inc(&[SERVICE_NAME, route.as_str(), status.as_str()], 1);
    handler_latency_seconds().observe(&[SERVICE_NAME, route.as_str()], latency);

    response
}

impl AuditWriter {
    async fn new(path: PathBuf) -> Result<Self, AuditError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }

        let prev_hash = Self::hydrate_prev_hash(&path).await?;
        Ok(Self { path, prev_hash })
    }

    async fn hydrate_prev_hash(path: &PathBuf) -> Result<Vec<u8>, AuditError> {
        if !path.exists() {
            return Ok(vec![0u8; 32]);
        }
        let contents = fs::read(path).await?;
        if contents.is_empty() {
            return Ok(vec![0u8; 32]);
        }
        let mut prev = vec![0u8; 32];
        for line in contents
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
        {
            let record: AuditRecord =
                serde_json::from_slice(line).map_err(|_| AuditError::Malformed)?;
            prev = STANDARD
                .decode(record.hash)
                .map_err(|_| AuditError::Malformed)?;
        }
        Ok(prev)
    }

    async fn append(&mut self, event: IncomingEvent) -> Result<(), AuditError> {
        let timestamp = Utc::now();
        let mut hasher = Sha256::new();
        hasher.update(&self.prev_hash);
        hasher.update(serde_json::to_vec(&event).map_err(|_| AuditError::Malformed)?);
        let hash = hasher.finalize();
        let record = AuditRecord {
            timestamp,
            prev_hash: STANDARD.encode(&self.prev_hash),
            hash: STANDARD.encode(&hash),
            event,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(
            serde_json::to_vec(&record)
                .map_err(|_| AuditError::Malformed)?
                .as_slice(),
        )
        .await?;
        file.write_all(b"\n").await?;
        self.prev_hash = hash.to_vec();
        Ok(())
    }

    async fn read_all(&self) -> Result<Vec<AuditRecord>, AuditError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read(&self.path).await?;
        let mut records = Vec::new();
        for line in contents
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
        {
            let record: AuditRecord =
                serde_json::from_slice(line).map_err(|_| AuditError::Malformed)?;
            records.push(record);
        }
        Ok(records)
    }
}
