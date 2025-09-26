use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use common_config::{load, MsgBusConfig, ServiceConfig};
use common_mdns::announce;
use common_msgbus::{MessageBus, NatsBus, NatsConfig};
use common_obs::{
    encode_prometheus_metrics, http_request_observe, msgbus_publish_total, ObsInit,
    PROMETHEUS_CONTENT_TYPE,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;

use std::time::Instant;

const SERVICE_NAME: &str = "radio-coord";
type SharedState = Arc<AppState>;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let config = load::<RadioCoordConfig>()?;
    let addr = config.socket_addr()?;
    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = build_sha(),
        build_time = build_time(),
        listen_addr = %addr,
        "starting service"
    );

    let bus_config = NatsConfig {
        url: config.bus.url.clone(),
        request_timeout: config.bus.request_timeout(),
    };
    let bus: Arc<dyn MessageBus> = Arc::new(NatsBus::connect(bus_config).await?);

    let _mdns = if config.announce_mdns {
        Some(announce(&config.mdns_service, config.port).await?)
    } else {
        tracing::info!(service = SERVICE_NAME, "mDNS announcement disabled");
        None
    };

    let state = Arc::new(AppState::new(bus));

    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/thread/dataset", post(apply_thread_dataset))
        .route("/v1/thread/channel", post(update_thread_channel))
        .route("/v1/wifi/config", post(apply_wifi_config))
        .route("/v1/wifi/channel", post(update_wifi_channel))
        .route("/v1/diag/radio-map", get(radio_map))
        .route("/metrics", get(metrics))
        .with_state(state.clone())
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": SERVICE_NAME }))
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

    http_request_observe!(route.as_str(), status.as_str(), latency);

    response
}

#[derive(Clone)]
struct AppState {
    bus: Arc<dyn MessageBus>,
    radio_map: Arc<RwLock<RadioMapSnapshot>>,
}

impl AppState {
    fn new(bus: Arc<dyn MessageBus>) -> Self {
        Self {
            bus,
            radio_map: Arc::new(RwLock::new(RadioMapSnapshot::default())),
        }
    }

    fn snapshot(&self) -> RadioMapSnapshot {
        self.radio_map.read().clone()
    }

    fn update_thread_dataset(&self, request: &ThreadDatasetRequest) {
        let now = Utc::now();
        let mut map = self.radio_map.write();
        map.thread.dataset = Some(ThreadDatasetSnapshot {
            dataset_id: request.dataset_id.clone(),
            network_name: request.network_name.clone(),
            channel: request.channel,
            pan_id: request.pan_id.clone(),
            xpan_id: request.xpan_id.clone(),
            updated_at: now,
        });
        map.thread.channel = Some(ThreadChannelSnapshot {
            channel: request.channel,
            dataset_id: Some(request.dataset_id.clone()),
            updated_at: now,
        });
    }

    fn update_thread_channel(&self, request: &ThreadChannelRequest) {
        let now = Utc::now();
        let mut map = self.radio_map.write();
        map.thread.channel = Some(ThreadChannelSnapshot {
            channel: request.channel,
            dataset_id: request.dataset_id.clone(),
            updated_at: now,
        });
        if let Some(dataset) = map.thread.dataset.as_mut() {
            dataset.channel = request.channel;
            dataset.updated_at = now;
        }
    }

    fn update_wifi_config(&self, request: &WifiConfigRequest) {
        let now = Utc::now();
        let mut map = self.radio_map.write();
        map.wifi.config = Some(WifiConfigSnapshot {
            ssid: request.ssid.clone(),
            security: request.security.as_str().to_string(),
            band: request.band.clone(),
            channel: request.channel,
            updated_at: now,
        });
        if let Some(channel) = request.channel {
            map.wifi.channel = Some(WifiChannelSnapshot {
                channel,
                band: request.band.clone(),
                updated_at: now,
            });
        }
    }

    fn update_wifi_channel(&self, request: &WifiChannelRequest) {
        let now = Utc::now();
        let mut map = self.radio_map.write();
        map.wifi.channel = Some(WifiChannelSnapshot {
            channel: request.channel,
            band: request.band.clone(),
            updated_at: now,
        });
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct RadioMapSnapshot {
    thread: ThreadSnapshot,
    wifi: WifiSnapshot,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnapshot {
    dataset: Option<ThreadDatasetSnapshot>,
    channel: Option<ThreadChannelSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadDatasetSnapshot {
    dataset_id: String,
    network_name: String,
    channel: u8,
    pan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    xpan_id: Option<String>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadChannelSnapshot {
    channel: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    dataset_id: Option<String>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct WifiSnapshot {
    config: Option<WifiConfigSnapshot>,
    channel: Option<WifiChannelSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WifiConfigSnapshot {
    ssid: String,
    security: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    band: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<u8>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WifiChannelSnapshot {
    channel: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    band: Option<String>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadDatasetRequest {
    dataset_id: String,
    network_name: String,
    channel: u8,
    pan_id: String,
    #[serde(default)]
    xpan_id: Option<String>,
    #[serde(default)]
    master_key: Option<String>,
    #[serde(default)]
    pskc: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadChannelRequest {
    channel: u8,
    #[serde(default)]
    dataset_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WifiConfigRequest {
    ssid: String,
    #[serde(default)]
    passphrase: Option<String>,
    #[serde(default)]
    security: WifiSecurity,
    #[serde(default)]
    band: Option<String>,
    #[serde(default)]
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WifiChannelRequest {
    channel: u8,
    #[serde(default)]
    band: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Acknowledgement {
    accepted: bool,
    message: String,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum WifiSecurity {
    Open,
    Wpa2,
    Wpa3,
}

impl Default for WifiSecurity {
    fn default() -> Self {
        WifiSecurity::Wpa2
    }
}

impl WifiSecurity {
    fn as_str(&self) -> &'static str {
        match self {
            WifiSecurity::Open => "open",
            WifiSecurity::Wpa2 => "wpa2",
            WifiSecurity::Wpa3 => "wpa3",
        }
    }
}

#[derive(Debug)]
enum ApiError {
    Validation(String),
    Bus(String),
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            ApiError::Validation(message) => (StatusCode::BAD_REQUEST, "validation_error", message),
            ApiError::Bus(message) => (StatusCode::SERVICE_UNAVAILABLE, "bus_error", message),
        };

        let body = Json(json!({
            "error": {
                "code": code,
                "message": message,
            }
        }));
        (status, body).into_response()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct RadioCoordConfig {
    pub bind_address: String,
    pub port: u16,
    pub announce_mdns: bool,
    pub mdns_service: String,
    #[serde(flatten)]
    pub bus: MsgBusConfig,
}

impl Default for RadioCoordConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8009,
            announce_mdns: true,
            mdns_service: "_lokan._tcp".to_string(),
            bus: MsgBusConfig::default(),
        }
    }
}

impl ServiceConfig for RadioCoordConfig {
    const PREFIX: &'static str = "RADIO_COORD_";

    fn apply_environment_overrides(&mut self, prefix: &str) {
        self.bus.apply_environment_overrides(prefix);
    }
}

impl RadioCoordConfig {
    fn socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.bind_address, self.port).parse()
    }
}

async fn apply_thread_dataset(
    State(state): State<SharedState>,
    Json(request): Json<ThreadDatasetRequest>,
) -> Result<(StatusCode, Json<Acknowledgement>), ApiError> {
    validate_thread_dataset(&request)?;

    let event = json!({
        "action": "thread.dataset.apply",
        "datasetId": request.dataset_id,
        "networkName": request.network_name,
        "channel": request.channel,
        "panId": request.pan_id,
        "xpanId": request.xpan_id,
    });

    publish_event(&state, "radio.thread.dataset.set", &event).await?;
    state.update_thread_dataset(&request);

    Ok((
        StatusCode::ACCEPTED,
        Json(Acknowledgement {
            accepted: true,
            message: "thread dataset accepted".to_string(),
        }),
    ))
}

async fn update_thread_channel(
    State(state): State<SharedState>,
    Json(request): Json<ThreadChannelRequest>,
) -> Result<(StatusCode, Json<Acknowledgement>), ApiError> {
    validate_thread_channel(&request)?;

    let event = json!({
        "action": "thread.channel.update",
        "channel": request.channel,
        "datasetId": request.dataset_id,
    });

    publish_event(&state, "radio.thread.channel.set", &event).await?;
    state.update_thread_channel(&request);

    Ok((
        StatusCode::ACCEPTED,
        Json(Acknowledgement {
            accepted: true,
            message: "thread channel update accepted".to_string(),
        }),
    ))
}

async fn apply_wifi_config(
    State(state): State<SharedState>,
    Json(request): Json<WifiConfigRequest>,
) -> Result<(StatusCode, Json<Acknowledgement>), ApiError> {
    validate_wifi_config(&request)?;

    let event = json!({
        "action": "wifi.config.apply",
        "ssid": request.ssid,
        "security": request.security.as_str(),
        "band": request.band,
        "channel": request.channel,
    });

    publish_event(&state, "radio.wifi.config.set", &event).await?;
    state.update_wifi_config(&request);

    Ok((
        StatusCode::ACCEPTED,
        Json(Acknowledgement {
            accepted: true,
            message: "wifi configuration accepted".to_string(),
        }),
    ))
}

async fn update_wifi_channel(
    State(state): State<SharedState>,
    Json(request): Json<WifiChannelRequest>,
) -> Result<(StatusCode, Json<Acknowledgement>), ApiError> {
    validate_wifi_channel(&request)?;

    let event = json!({
        "action": "wifi.channel.update",
        "channel": request.channel,
        "band": request.band,
    });

    publish_event(&state, "radio.wifi.channel.set", &event).await?;
    state.update_wifi_channel(&request);

    Ok((
        StatusCode::ACCEPTED,
        Json(Acknowledgement {
            accepted: true,
            message: "wifi channel update accepted".to_string(),
        }),
    ))
}

fn validate_thread_dataset(request: &ThreadDatasetRequest) -> Result<(), ApiError> {
    ensure_hex(&request.dataset_id, 32, "datasetId")?;
    ensure_name(&request.network_name, 1, 16, "networkName")?;
    ensure_thread_channel(request.channel)?;
    ensure_hex(&request.pan_id, 4, "panId")?;
    if let Some(xpan_id) = &request.xpan_id {
        ensure_hex(xpan_id, 16, "xpanId")?;
    }
    if let Some(master_key) = &request.master_key {
        ensure_hex(master_key, 32, "masterKey")?;
    }
    if let Some(pskc) = &request.pskc {
        ensure_hex(pskc, 32, "pskc")?;
    }
    Ok(())
}

fn validate_thread_channel(request: &ThreadChannelRequest) -> Result<(), ApiError> {
    ensure_thread_channel(request.channel)?;
    if let Some(dataset_id) = &request.dataset_id {
        ensure_hex(dataset_id, 32, "datasetId")?;
    }
    Ok(())
}

fn validate_wifi_config(request: &WifiConfigRequest) -> Result<(), ApiError> {
    ensure_name(&request.ssid, 1, 32, "ssid")?;
    if matches!(request.security, WifiSecurity::Wpa2 | WifiSecurity::Wpa3) {
        let passphrase = request.passphrase.as_ref().ok_or_else(|| {
            ApiError::Validation("passphrase is required for secured networks".to_string())
        })?;
        if passphrase.len() < 8 || passphrase.len() > 63 {
            return Err(ApiError::Validation(
                "passphrase must be between 8 and 63 characters".to_string(),
            ));
        }
    }

    if let Some(band) = &request.band {
        ensure_band(band)?;
    }

    if let Some(channel) = request.channel {
        ensure_wifi_channel(channel)?;
    }

    Ok(())
}

fn validate_wifi_channel(request: &WifiChannelRequest) -> Result<(), ApiError> {
    ensure_wifi_channel(request.channel)?;
    if let Some(band) = &request.band {
        ensure_band(band)?;
    }
    Ok(())
}

fn ensure_thread_channel(channel: u8) -> Result<(), ApiError> {
    if (11..=26).contains(&channel) {
        Ok(())
    } else {
        Err(ApiError::Validation(format!(
            "thread channel {} is outside the 11-26 range",
            channel
        )))
    }
}

fn ensure_wifi_channel(channel: u8) -> Result<(), ApiError> {
    if (1..=165).contains(&channel) {
        Ok(())
    } else {
        Err(ApiError::Validation(format!(
            "wifi channel {} is outside the 1-165 range",
            channel
        )))
    }
}

fn ensure_band(band: &str) -> Result<(), ApiError> {
    let normalized = band.to_ascii_lowercase();
    match normalized.as_str() {
        "2.4ghz" | "5ghz" | "6ghz" | "dual" => Ok(()),
        _ => Err(ApiError::Validation(format!(
            "unsupported band '{}'; expected 2.4GHz, 5GHz, 6GHz, or dual",
            band
        ))),
    }
}

fn ensure_hex(value: &str, expected_len: usize, field: &str) -> Result<(), ApiError> {
    if value.len() != expected_len {
        return Err(ApiError::Validation(format!(
            "{} must be {} hexadecimal characters",
            field, expected_len
        )));
    }
    if value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ApiError::Validation(format!(
            "{} must contain only hexadecimal characters",
            field
        )))
    }
}

fn ensure_name(value: &str, min: usize, max: usize, field: &str) -> Result<(), ApiError> {
    if value.len() < min || value.len() > max {
        return Err(ApiError::Validation(format!(
            "{} must be between {} and {} characters",
            field, min, max
        )));
    }
    if value.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        Ok(())
    } else {
        Err(ApiError::Validation(format!(
            "{} must contain printable ASCII characters",
            field
        )))
    }
}

async fn publish_event(
    state: &AppState,
    subject: &str,
    payload: &serde_json::Value,
) -> Result<(), ApiError> {
    let bytes = serde_json::to_vec(payload).map_err(|err| ApiError::Bus(err.to_string()))?;
    msgbus_publish_total().inc(&[SERVICE_NAME, subject], 1);
    state
        .bus
        .publish(subject, &bytes)
        .await
        .map_err(|err| ApiError::Bus(err.to_string()))
}

async fn radio_map(State(state): State<SharedState>) -> Json<RadioMapSnapshot> {
    Json(state.snapshot())
}
