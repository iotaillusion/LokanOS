use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use common_config::service_port;
use common_obs::health_router;

use base64::{engine::general_purpose::STANDARD, Engine as _};

const SERVICE_NAME: &str = "audit-log";
const PORT_ENV: &str = "AUDIT_LOG_PORT";
const DEFAULT_PORT: u16 = 8008;
const DEFAULT_PATH: &str = "audit.log";

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
    init_tracing();

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let log_path = std::env::var("AUDIT_LOG_PATH").unwrap_or_else(|_| DEFAULT_PATH.to_string());

    let writer = AuditWriter::new(PathBuf::from(&log_path)).await?;
    let state = AppState {
        writer: Arc::new(Mutex::new(writer)),
    };

    tracing::info!(%addr, %log_path, service = SERVICE_NAME, "starting service");

    let app = Router::new()
        .route("/v1/events", post(record_event))
        .route("/v1/events/export", get(export_events))
        .with_state(state)
        .merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
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
