use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, health_router, http_requests_total,
    ObsInit, PROMETHEUS_CONTENT_TYPE,
};

use std::time::Instant;

const SERVICE_NAME: &str = "presence-svc";
const PORT_ENV: &str = "PRESENCE_SVC_PORT";
const DEFAULT_PORT: u16 = 8004;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[derive(Clone)]
struct AppState {
    events: broadcast::Sender<PresenceEvent>,
}

#[derive(Debug, Clone, Deserialize)]
struct WebhookPayload {
    person_id: String,
    location: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default = "now_ts")]
    observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct BlePayload {
    beacon_id: String,
    rssi: i32,
    room_hint: Option<String>,
    #[serde(default = "now_ts")]
    observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct PresenceEvent {
    source: PresenceSource,
    person_id: String,
    location: String,
    confidence: f32,
    observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum PresenceSource {
    Webhook,
    Ble,
}

fn default_confidence() -> f32 {
    0.5
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let (tx, _) = broadcast::channel(128);
    let state = AppState { events: tx };

    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = build_sha(),
        build_time = build_time(),
        listen_addr = %addr,
        "starting service"
    );

    let app = Router::new()
        .route("/v1/presence/webhook", post(intake_webhook))
        .route("/v1/presence/ble", post(intake_ble))
        .route("/v1/presence/events", get(stream_events))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn intake_webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    let event = PresenceEvent {
        source: PresenceSource::Webhook,
        person_id: payload.person_id,
        location: payload.location,
        confidence: payload.confidence,
        observed_at: payload.observed_at,
    };
    dispatch_event(&state.events, event);
    StatusCode::ACCEPTED
}

async fn intake_ble(State(state): State<AppState>, Json(payload): Json<BlePayload>) -> StatusCode {
    let confidence = 1.0_f32.min(0.2 + ((-payload.rssi) as f32).abs() / 100.0);
    let event = PresenceEvent {
        source: PresenceSource::Ble,
        person_id: payload.beacon_id.clone(),
        location: payload
            .room_hint
            .unwrap_or_else(|| format!("beacon:{}", payload.beacon_id)),
        confidence,
        observed_at: payload.observed_at,
    };
    dispatch_event(&state.events, event);
    StatusCode::ACCEPTED
}

async fn stream_events(
    State(state): State<AppState>,
) -> axum::response::Sse<impl Stream<Item = Result<Event, anyhow::Error>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|event| async {
        match event {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(payload) => Some(Ok(Event::default().data(payload))),
                Err(err) => Some(Err(anyhow::anyhow!(err))),
            },
            Err(_) => None,
        }
    });
    axum::response::Sse::new(stream).keep_alive(KeepAlive::new())
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

fn dispatch_event(sender: &broadcast::Sender<PresenceEvent>, event: PresenceEvent) {
    tracing::info!(person = %event.person_id, location = %event.location, source = ?event.source, "presence event");
    let _ = sender.send(event);
}

fn now_ts() -> DateTime<Utc> {
    Utc::now()
}
