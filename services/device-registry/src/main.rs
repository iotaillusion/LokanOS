use axum::body::Body;
use axum::extract::{ws::Message, MatchedPath, Path, State, WebSocketUpgrade};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, put};
use axum::{Json, Router};
use futures_core::Stream;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use sqlx::Row;

use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, health_router, http_requests_total,
    ObsInit, PROMETHEUS_CONTENT_TYPE,
};

use std::time::Instant;

#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("enable only one backend feature at a time");

type DbPool = sqlx::AnyPool;

const SERVICE_NAME: &str = "device-registry";
const PORT_ENV: &str = "DEVICE_REGISTRY_PORT";
const DEFAULT_PORT: u16 = 8001;
#[cfg(feature = "postgres")]
const DEFAULT_DB_URL: &str = "postgres://localhost/device_registry";
#[cfg(not(feature = "postgres"))]
const DEFAULT_DB_URL: &str = "sqlite://device-registry.db";

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[derive(Clone)]
struct AppState {
    pool: DbPool,
    events: broadcast::Sender<DeviceEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct DeviceEvent {
    kind: EventKind,
    device_id: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum EventKind {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Room {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Device {
    id: String,
    room_id: Option<String>,
    name: String,
    kind: String,
    status: String,
    state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Capability {
    id: i64,
    device_id: String,
    capability: String,
    properties: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct NewRoom {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct NewDevice {
    room_id: Option<String>,
    name: String,
    kind: String,
    #[serde(default = "default_status")]
    status: String,
    #[serde(default)]
    state: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateDevice {
    room_id: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CapabilityPayload {
    capability: String,
    #[serde(default)]
    properties: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
enum RegistryError {
    #[error("record not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for RegistryError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            RegistryError::NotFound => StatusCode::NOT_FOUND,
            RegistryError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let msg = self.to_string();
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let database_url = std::env::var("DEVICE_REGISTRY_DATABASE_URL")
        .unwrap_or_else(|_| DEFAULT_DB_URL.to_string());

    let pool = init_pool(&database_url).await?;
    init_schema(&pool).await?;

    let (tx, _) = broadcast::channel(64);
    let state = AppState { pool, events: tx };

    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = build_sha(),
        build_time = build_time(),
        listen_addr = %addr,
        db = %database_url,
        "starting service"
    );

    let app = Router::new()
        .route("/v1/rooms", get(list_rooms).post(create_room))
        .route("/v1/devices", get(list_devices).post(create_device))
        .route(
            "/v1/devices/:id",
            get(fetch_device).put(update_device).delete(delete_device),
        )
        .route("/v1/devices/:id/state", put(update_device_state))
        .route(
            "/v1/devices/:id/capabilities",
            get(list_capabilities).post(add_capability),
        )
        .route("/v1/events/sse", get(events_sse))
        .route("/v1/events/ws", get(events_ws))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn init_pool(url: &str) -> Result<DbPool, sqlx::Error> {
    sqlx::AnyPool::connect(url).await
}

async fn init_schema(pool: &DbPool) -> Result<(), sqlx::Error> {
    #[cfg(feature = "postgres")]
    let create_capabilities = "CREATE TABLE IF NOT EXISTS capabilities (
        id SERIAL PRIMARY KEY,
        device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
        capability TEXT NOT NULL,
        properties TEXT NOT NULL
    )";

    #[cfg(not(feature = "postgres"))]
    let create_capabilities = "CREATE TABLE IF NOT EXISTS capabilities (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
        capability TEXT NOT NULL,
        properties TEXT NOT NULL
    )";

    let create_rooms = "CREATE TABLE IF NOT EXISTS rooms (id TEXT PRIMARY KEY, name TEXT NOT NULL)";
    let create_devices = "CREATE TABLE IF NOT EXISTS devices (
        id TEXT PRIMARY KEY,
        room_id TEXT REFERENCES rooms(id) ON DELETE SET NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        status TEXT NOT NULL,
        state TEXT NOT NULL
    )";
    sqlx::query(create_rooms).execute(pool).await?;
    sqlx::query(create_devices).execute(pool).await?;
    sqlx::query(create_capabilities).execute(pool).await?;
    Ok(())
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

async fn list_rooms(State(state): State<AppState>) -> Result<Json<Vec<Room>>, RegistryError> {
    let rows = sqlx::query("SELECT id, name FROM rooms ORDER BY name")
        .fetch_all(&state.pool)
        .await?;

    let rooms = rows
        .into_iter()
        .map(|row| Room {
            id: row.get("id"),
            name: row.get("name"),
        })
        .collect();
    Ok(Json(rooms))
}

async fn create_room(
    State(state): State<AppState>,
    Json(payload): Json<NewRoom>,
) -> Result<Json<Room>, RegistryError> {
    let id = Uuid::new_v4().to_string();
    let name = payload.name;
    sqlx::query("INSERT INTO rooms (id, name) VALUES (?, ?)")
        .bind(&id)
        .bind(&name)
        .execute(&state.pool)
        .await?;
    let room = Room { id, name };
    Ok(Json(room))
}

async fn list_devices(State(state): State<AppState>) -> Result<Json<Vec<Device>>, RegistryError> {
    let rows =
        sqlx::query("SELECT id, room_id, name, kind, status, state FROM devices ORDER BY name")
            .fetch_all(&state.pool)
            .await?;
    let devices = rows
        .into_iter()
        .map(|row| Device {
            id: row.get("id"),
            room_id: row.get("room_id"),
            name: row.get("name"),
            kind: row.get("kind"),
            status: row.get("status"),
            state: parse_state(row.get("state")),
        })
        .collect();
    Ok(Json(devices))
}

async fn create_device(
    State(state): State<AppState>,
    Json(payload): Json<NewDevice>,
) -> Result<Json<Device>, RegistryError> {
    let id = Uuid::new_v4().to_string();
    let room_id = payload.room_id.clone();
    let name = payload.name.clone();
    let kind = payload.kind.clone();
    let status = payload.status.clone();
    let state_value = payload.state.clone();
    let state_json = serde_json::to_string(&payload.state).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO devices (id, room_id, name, kind, status, state) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&room_id)
    .bind(&name)
    .bind(&kind)
    .bind(&status)
    .bind(state_json)
    .execute(&state.pool)
    .await?;

    let device = Device {
        id: id.clone(),
        room_id,
        name,
        kind,
        status,
        state: state_value,
    };

    publish_event(&state.events, EventKind::Created, &id, &device);
    Ok(Json(device))
}

async fn fetch_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Device>, RegistryError> {
    let record =
        sqlx::query("SELECT id, room_id, name, kind, status, state FROM devices WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await?;

    let record = record.ok_or(RegistryError::NotFound)?;
    let device = Device {
        id: record.get("id"),
        room_id: record.get("room_id"),
        name: record.get("name"),
        kind: record.get("kind"),
        status: record.get("status"),
        state: parse_state(record.get("state")),
    };
    Ok(Json(device))
}

async fn update_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateDevice>,
) -> Result<Json<Device>, RegistryError> {
    let existing = fetch_device(State(state.clone()), Path(id.clone()))
        .await?
        .0;
    let updated_name = payload.name.unwrap_or(existing.name.clone());
    let updated_kind = payload.kind.unwrap_or(existing.kind.clone());
    let updated_status = payload.status.unwrap_or(existing.status.clone());
    let updated_state = payload.state.unwrap_or(existing.state.clone());

    sqlx::query(
        "UPDATE devices SET room_id = ?, name = ?, kind = ?, status = ?, state = ? WHERE id = ?",
    )
    .bind(payload.room_id.clone().or(existing.room_id.clone()))
    .bind(&updated_name)
    .bind(&updated_kind)
    .bind(&updated_status)
    .bind(serde_json::to_string(&updated_state).unwrap_or_else(|_| existing.state.to_string()))
    .bind(&id)
    .execute(&state.pool)
    .await?;

    let device = Device {
        id: id.clone(),
        room_id: payload.room_id.or(existing.room_id),
        name: updated_name,
        kind: updated_kind,
        status: updated_status,
        state: updated_state,
    };

    publish_event(&state.events, EventKind::Updated, &id, &device);
    Ok(Json(device))
}

async fn update_device_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(new_state): Json<serde_json::Value>,
) -> Result<Json<Device>, RegistryError> {
    let existing = fetch_device(State(state.clone()), Path(id.clone()))
        .await?
        .0;
    sqlx::query("UPDATE devices SET state = ? WHERE id = ?")
        .bind(serde_json::to_string(&new_state).unwrap_or_else(|_| existing.state.to_string()))
        .bind(&id)
        .execute(&state.pool)
        .await?;

    let device = Device {
        state: new_state.clone(),
        ..existing
    };
    publish_event(&state.events, EventKind::Updated, &device.id, &device);
    Ok(Json(device))
}

async fn delete_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, RegistryError> {
    let result = sqlx::query("DELETE FROM devices WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(RegistryError::NotFound);
    }
    publish_event(
        &state.events,
        EventKind::Deleted,
        &id,
        &serde_json::json!({}),
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn list_capabilities(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Capability>>, RegistryError> {
    let records = sqlx::query(
        "SELECT id, device_id, capability, properties FROM capabilities WHERE device_id = ?",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;

    let caps = records
        .into_iter()
        .map(|row| Capability {
            id: row.get("id"),
            device_id: row.get("device_id"),
            capability: row.get("capability"),
            properties: parse_state(row.get("properties")),
        })
        .collect();
    Ok(Json(caps))
}

async fn add_capability(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CapabilityPayload>,
) -> Result<Json<Capability>, RegistryError> {
    let properties =
        serde_json::to_string(&payload.properties).unwrap_or_else(|_| "{}".to_string());
    sqlx::query("INSERT INTO capabilities (device_id, capability, properties) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(&payload.capability)
        .bind(&properties)
        .execute(&state.pool)
        .await?;
    let record = sqlx::query(
        "SELECT id FROM capabilities WHERE device_id = ? AND capability = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(&id)
    .bind(&payload.capability)
    .fetch_one(&state.pool)
    .await?;
    let cap_id: i64 = record.get("id");
    let capability = Capability {
        id: cap_id,
        device_id: id.clone(),
        capability: payload.capability,
        properties: payload.properties,
    };
    publish_event(
        &state.events,
        EventKind::Updated,
        &id,
        &serde_json::json!({ "capability": capability.capability }),
    );
    Ok(Json(capability))
}

async fn events_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|event| async {
        match event {
            Ok(ev) => match serde_json::to_string(&ev) {
                Ok(payload) => Some(Ok(Event::default().data(payload))),
                Err(err) => Some(Err(anyhow::anyhow!(err))),
            },
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new())
}

async fn events_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let mut receiver = BroadcastStream::new(state.events.subscribe()).fuse();
        let (mut tx, mut rx) = socket.split();
        tokio::spawn(async move {
            while let Some(event) = receiver.next().await {
                if let Ok(event) = event {
                    if let Ok(payload) = serde_json::to_string(&event) {
                        if tx.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // drain incoming messages to keep connection alive
        while let Some(Ok(msg)) = rx.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    })
}

fn publish_event<T: Serialize>(
    sender: &broadcast::Sender<DeviceEvent>,
    kind: EventKind,
    device_id: &str,
    payload: T,
) {
    if let Ok(payload) = serde_json::to_value(payload) {
        let event = DeviceEvent {
            kind,
            device_id: device_id.to_string(),
            payload,
        };
        let _ = sender.send(event);
    }
}

fn default_status() -> String {
    "unknown".to_string()
}

fn parse_state(raw: String) -> serde_json::Value {
    serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
}
