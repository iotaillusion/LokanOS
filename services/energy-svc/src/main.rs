use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{MatchedPath, Query, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, health_router, http_request_observe, ObsInit,
    PROMETHEUS_CONTENT_TYPE,
};

use std::time::Instant;

const SERVICE_NAME: &str = "energy-svc";
const PORT_ENV: &str = "ENERGY_SVC_PORT";
const DEFAULT_PORT: u16 = 8005;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[derive(Clone)]
struct AppState {
    state: Arc<RwLock<EnergyState>>,
}

#[derive(Clone, Default)]
struct EnergyState {
    budgets: HashMap<String, EnergyBudget>,
    tou_windows: Vec<TimeOfUseWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnergyBudget {
    id: String,
    limit_kwh: f32,
    #[serde(default)]
    period: BudgetPeriod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetPeriod {
    Daily,
    Weekly,
    Monthly,
}

impl Default for BudgetPeriod {
    fn default() -> Self {
        Self::Daily
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimeOfUseWindow {
    name: String,
    start: String,
    end: String,
    #[serde(default = "default_multiplier")]
    rate_multiplier: f32,
}

#[derive(Debug, Deserialize)]
struct AdviceQuery {
    #[serde(default)]
    consumption_kwh: f32,
}

#[derive(Debug, Serialize)]
struct AdviceResponse {
    timestamp: DateTime<Utc>,
    summary: String,
    recommendations: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let state = AppState {
        state: Arc::new(RwLock::new(EnergyState::default())),
    };

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
        .route("/v1/budgets", post(set_budgets))
        .route("/v1/tou", post(set_tou_windows))
        .route("/v1/advice", get(get_advice))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn set_budgets(
    State(state): State<AppState>,
    Json(budgets): Json<Vec<EnergyBudget>>,
) -> Json<StatusReply> {
    let mut guard = state.state.write().await;
    guard.budgets = budgets
        .into_iter()
        .map(|budget| (budget.id.clone(), budget))
        .collect();
    StatusReply::ok("budgets updated")
}

async fn set_tou_windows(
    State(state): State<AppState>,
    Json(windows): Json<Vec<TimeOfUseWindow>>,
) -> Json<StatusReply> {
    let mut guard = state.state.write().await;
    guard.tou_windows = windows;
    StatusReply::ok("time-of-use windows updated")
}

async fn get_advice(
    State(state): State<AppState>,
    Query(query): Query<AdviceQuery>,
) -> Json<AdviceResponse> {
    let snapshot = state.state.read().await.clone();
    let now_local: DateTime<Local> = Local::now();
    let current_time = now_local.time();

    let mut recommendations = Vec::new();
    for window in &snapshot.tou_windows {
        if let Some((start, end)) = parse_window(window) {
            if in_window(current_time, start, end) && window.rate_multiplier > 1.0 {
                recommendations.push(format!(
                    "High rate period ({}) active. Consider delaying discretionary loads.",
                    window.name
                ));
            }
        }
    }

    for budget in snapshot.budgets.values() {
        if query.consumption_kwh > budget.limit_kwh {
            recommendations.push(format!(
                "{} budget exceeded by {:.2} kWh. Reduce usage or reschedule appliances.",
                match budget.period {
                    BudgetPeriod::Daily => "Daily",
                    BudgetPeriod::Weekly => "Weekly",
                    BudgetPeriod::Monthly => "Monthly",
                },
                query.consumption_kwh - budget.limit_kwh
            ));
        }
    }

    if recommendations.is_empty() {
        recommendations.push("Usage within configured thresholds.".to_string());
    }

    Json(AdviceResponse {
        timestamp: Utc::now(),
        summary: format!("Current consumption {:.2} kWh", query.consumption_kwh),
        recommendations,
    })
}

fn parse_window(window: &TimeOfUseWindow) -> Option<(NaiveTime, NaiveTime)> {
    let start = NaiveTime::parse_from_str(&window.start, "%H:%M").ok()?;
    let end = NaiveTime::parse_from_str(&window.end, "%H:%M").ok()?;
    Some((start, end))
}

fn in_window(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start <= end {
        now >= start && now <= end
    } else {
        now >= start || now <= end
    }
}

fn default_multiplier() -> f32 {
    1.0
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

#[derive(Debug, Serialize)]
struct StatusReply {
    message: String,
}

impl StatusReply {
    fn ok(message: &str) -> Json<Self> {
        Json(Self {
            message: message.to_string(),
        })
    }
}
