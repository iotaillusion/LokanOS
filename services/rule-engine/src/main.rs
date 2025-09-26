use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{MatchedPath, Path, Query, State};
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use uuid::Uuid;

use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, health_router, http_requests_total,
    ObsInit, PROMETHEUS_CONTENT_TYPE,
};

use std::time::Instant;

const SERVICE_NAME: &str = "rule-engine";
const PORT_ENV: &str = "RULE_ENGINE_PORT";
const DEFAULT_PORT: u16 = 8002;
const TICK_INTERVAL_MS: u64 = 500;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

const MAX_TRACE_ENTRIES: usize = 100;

#[derive(Clone)]
struct AppState {
    rules: Arc<RwLock<HashMap<String, RuleInstance>>>,
    traces: Arc<RwLock<HashMap<String, VecDeque<RuleTraceEntry>>>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            traces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn record_trace(&self, rule_id: &str, entry: RuleTraceEntry) {
        let mut guard = self.traces.write();
        let deque = guard.entry(rule_id.to_string()).or_default();
        if deque.len() == MAX_TRACE_ENTRIES {
            deque.pop_front();
        }
        deque.push_back(entry);
    }

    fn traces_for(&self, rule_id: &str) -> Option<Vec<RuleTraceEntry>> {
        self.traces
            .read()
            .get(rule_id)
            .map(|entries| entries.iter().cloned().rev().collect())
    }

    fn init_trace_slot(&self, rule_id: &str) {
        let mut guard = self.traces.write();
        guard.entry(rule_id.to_string()).or_default();
    }

    fn drop_trace_slot(&self, rule_id: &str) {
        self.traces.write().remove(rule_id);
    }
}

struct RuleInstance {
    definition: RuleDefinition,
    schedule: ScheduleState,
}

#[derive(Clone)]
struct ScheduleState {
    next_tick: u64,
    interval_ticks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleDefinition {
    pub id: String,
    pub name: Option<String>,
    pub trigger: Trigger,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Trigger {
    Interval { seconds: u64 },
    Event { subject: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Condition {
    Equals { left: ValueRef, right: ValueRef },
    GreaterThan { left: ValueRef, right: ValueRef },
    LessThan { left: ValueRef, right: ValueRef },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ValueRef {
    Literal { value: serde_json::Value },
    Context { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Action {
    EmitEvent {
        subject: String,
        payload: serde_json::Value,
    },
    SetDeviceState {
        device_id: String,
        state: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct RuleTestRequest {
    rule: RuleDefinition,
    #[serde(default)]
    context: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct RuleTestResponse {
    fired: bool,
    trace: Vec<String>,
    actions: Vec<ActionExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionExecution {
    action: Action,
    status: ActionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActionStatus {
    Executed,
    Skipped,
}

#[derive(Debug, thiserror::Error)]
enum RuleEngineError {
    #[error("rule not found")]
    NotFound,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl axum::response::IntoResponse for RuleEngineError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            RuleEngineError::NotFound => StatusCode::NOT_FOUND,
            RuleEngineError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
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

    let state = AppState::new();
    let scheduler_state = state.clone();
    tokio::spawn(async move {
        run_scheduler(scheduler_state).await;
    });

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
        .route("/v1/rules", get(list_rules).post(create_rule))
        .route("/v1/rules/:id", delete(delete_rule))
        .route("/v1/rules:test", post(test_rule))
        .route("/v1/diag/trace", get(rule_trace))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn list_rules(State(state): State<AppState>) -> Json<Vec<RuleDefinition>> {
    let rules = state
        .rules
        .read()
        .values()
        .map(|instance| instance.definition.clone())
        .collect();
    Json(rules)
}

async fn create_rule(
    State(state): State<AppState>,
    Json(mut payload): Json<RuleDefinition>,
) -> Json<RuleDefinition> {
    if payload.id.is_empty() {
        payload.id = Uuid::new_v4().to_string();
    }
    state.init_trace_slot(&payload.id);
    let mut guard = state.rules.write();
    let ticks = guard
        .values()
        .map(|instance| instance.schedule.next_tick)
        .max()
        .unwrap_or(0);
    guard.insert(
        payload.id.clone(),
        RuleInstance {
            schedule: ScheduleState::new(&payload.trigger, ticks),
            definition: payload.clone(),
        },
    );
    Json(payload)
}

async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, RuleEngineError> {
    let mut guard = state.rules.write();
    if guard.remove(&id).is_some() {
        state.drop_trace_slot(&id);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RuleEngineError::NotFound)
    }
}

async fn test_rule(Json(request): Json<RuleTestRequest>) -> Json<RuleTestResponse> {
    let now = Utc::now();
    let evaluation = evaluate_rule(&request.rule, &request.context, now);
    Json(RuleTestResponse {
        fired: evaluation.fired,
        trace: evaluation.trace,
        actions: evaluation.actions,
    })
}

struct EvaluationResult {
    fired: bool,
    trace: Vec<String>,
    actions: Vec<ActionExecution>,
}

#[derive(Debug, Clone, Serialize)]
struct RuleTraceEntry {
    timestamp: DateTime<Utc>,
    fired: bool,
    duration_ms: f64,
    trace: Vec<String>,
    actions: Vec<ActionExecution>,
}

fn evaluate_rule(
    rule: &RuleDefinition,
    context: &serde_json::Map<String, serde_json::Value>,
    now: DateTime<Utc>,
) -> EvaluationResult {
    let mut trace = Vec::new();
    trace.push(format!("evaluating rule {}", rule.id));
    let mut conditions_met = true;

    for condition in &rule.conditions {
        match condition.evaluate(context, now) {
            ConditionState::Matched(msg) => trace.push(msg),
            ConditionState::Failed(msg) => {
                trace.push(msg);
                conditions_met = false;
                break;
            }
        }
    }

    let mut actions = Vec::new();
    if conditions_met {
        for action in &rule.actions {
            actions.push(ActionExecution {
                action: action.clone(),
                status: ActionStatus::Executed,
            });
        }
        trace.push("conditions satisfied".to_string());
    } else {
        for action in &rule.actions {
            actions.push(ActionExecution {
                action: action.clone(),
                status: ActionStatus::Skipped,
            });
        }
        trace.push("conditions failed".to_string());
    }

    EvaluationResult {
        fired: conditions_met,
        trace,
        actions,
    }
}

impl Condition {
    fn evaluate(
        &self,
        context: &serde_json::Map<String, serde_json::Value>,
        now: DateTime<Utc>,
    ) -> ConditionState {
        match self {
            Condition::Equals { left, right } => {
                let left_value = left.resolve(context, now);
                let right_value = right.resolve(context, now);
                if left_value == right_value {
                    ConditionState::Matched(format!(
                        "equals matched: {left_value:?} == {right_value:?}"
                    ))
                } else {
                    ConditionState::Failed(format!(
                        "equals failed: {left_value:?} != {right_value:?}"
                    ))
                }
            }
            Condition::GreaterThan { left, right } => {
                compare_numeric("greater_than", left, right, context, now, |l, r| l > r)
            }
            Condition::LessThan { left, right } => {
                compare_numeric("less_than", left, right, context, now, |l, r| l < r)
            }
        }
    }
}

fn compare_numeric<F: Fn(f64, f64) -> bool>(
    label: &str,
    left: &ValueRef,
    right: &ValueRef,
    context: &serde_json::Map<String, serde_json::Value>,
    now: DateTime<Utc>,
    cmp: F,
) -> ConditionState {
    let left_value = left.resolve(context, now);
    let right_value = right.resolve(context, now);
    match (left_value.as_f64(), right_value.as_f64()) {
        (Some(l), Some(r)) if cmp(l, r) => {
            ConditionState::Matched(format!("{label} matched: {l} vs {r}"))
        }
        (Some(l), Some(r)) => ConditionState::Failed(format!("{label} failed: {l} vs {r}")),
        _ => ConditionState::Failed(format!(
            "{label} failed: unable to coerce {left_value:?} or {right_value:?} to numbers"
        )),
    }
}

impl ValueRef {
    fn resolve(
        &self,
        context: &serde_json::Map<String, serde_json::Value>,
        now: DateTime<Utc>,
    ) -> serde_json::Value {
        match self {
            ValueRef::Literal { value } => value.clone(),
            ValueRef::Context { path } => {
                resolve_path(context, path).unwrap_or_else(|| match path.as_str() {
                    "now" => serde_json::Value::String(now.to_rfc3339()),
                    _ => serde_json::Value::Null,
                })
            }
        }
    }
}

fn resolve_path(
    context: &serde_json::Map<String, serde_json::Value>,
    path: &str,
) -> Option<serde_json::Value> {
    if let Some(value) = context.get(path) {
        return Some(value.clone());
    }
    let mut iter = path.split('.');
    let mut value = context.get(iter.next()?)?;
    for part in iter {
        match value {
            serde_json::Value::Object(map) => {
                value = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(value.clone())
}

enum ConditionState {
    Matched(String),
    Failed(String),
}

async fn run_scheduler(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_millis(TICK_INTERVAL_MS));
    let mut tick: u64 = 0;
    loop {
        interval.tick().await;
        tick = tick.wrapping_add(1);
        let now = Utc::now();
        let mut guard = state.rules.write();
        for instance in guard.values_mut() {
            if instance.schedule.should_fire(tick) {
                let mut context = serde_json::Map::new();
                context.insert(
                    "now".to_string(),
                    serde_json::Value::String(now.to_rfc3339()),
                );
                context.insert("tick".to_string(), serde_json::Value::Number(tick.into()));
                let started = Instant::now();
                let result = evaluate_rule(&instance.definition, &context, now);
                let duration = started.elapsed().as_secs_f64() * 1_000.0;
                let trace_entry = RuleTraceEntry {
                    timestamp: now,
                    fired: result.fired,
                    duration_ms: duration,
                    trace: result.trace.clone(),
                    actions: result.actions.clone(),
                };
                if result.fired {
                    tracing::info!(rule = %instance.definition.id, trace = ?result.trace, "rule fired");
                } else {
                    tracing::debug!(rule = %instance.definition.id, trace = ?result.trace, "rule skipped");
                }
                state.record_trace(&instance.definition.id, trace_entry);
                instance.schedule.advance();
            }
        }
    }
}

impl ScheduleState {
    fn new(trigger: &Trigger, current_tick: u64) -> Self {
        match trigger {
            Trigger::Interval { seconds } => {
                let secs = (*seconds).max(1);
                let interval_ms = secs * 1000;
                let interval_ticks =
                    ((interval_ms + TICK_INTERVAL_MS - 1) / TICK_INTERVAL_MS).max(1);
                Self {
                    next_tick: current_tick + interval_ticks,
                    interval_ticks,
                }
            }
            Trigger::Event { .. } => Self {
                next_tick: u64::MAX,
                interval_ticks: u64::MAX,
            },
        }
    }

    fn should_fire(&self, tick: u64) -> bool {
        tick >= self.next_tick
    }

    fn advance(&mut self) {
        if self.interval_ticks == u64::MAX {
            self.next_tick = u64::MAX;
        } else {
            self.next_tick = self.next_tick.saturating_add(self.interval_ticks);
        }
    }
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

#[derive(Debug, Deserialize)]
struct TraceQuery {
    rule_id: String,
}

#[derive(Debug, Serialize)]
struct TraceResponse {
    rule_id: String,
    executions: Vec<RuleTraceEntry>,
}

async fn rule_trace(
    State(state): State<AppState>,
    Query(params): Query<TraceQuery>,
) -> Result<Json<TraceResponse>, RuleEngineError> {
    if params.rule_id.trim().is_empty() {
        return Err(RuleEngineError::InvalidRequest(
            "rule_id query parameter is required".to_string(),
        ));
    }

    if !state.rules.read().contains_key(params.rule_id.as_str()) {
        return Err(RuleEngineError::NotFound);
    }

    let executions = state
        .traces_for(params.rule_id.as_str())
        .unwrap_or_default();

    Ok(Json(TraceResponse {
        rule_id: params.rule_id,
        executions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_rule_executes_when_conditions_match() {
        let rule = RuleDefinition {
            id: "rule-1".to_string(),
            name: None,
            trigger: Trigger::Interval { seconds: 1 },
            conditions: vec![Condition::Equals {
                left: ValueRef::Context {
                    path: "temperature".to_string(),
                },
                right: ValueRef::Literal {
                    value: serde_json::json!(72),
                },
            }],
            actions: vec![Action::EmitEvent {
                subject: "hvac.adjust".to_string(),
                payload: serde_json::json!({"target": 70}),
            }],
        };

        let mut context = serde_json::Map::new();
        context.insert("temperature".to_string(), serde_json::json!(72));
        let result = evaluate_rule(&rule, &context, Utc::now());
        assert!(result.fired);
        assert!(matches!(result.actions[0].status, ActionStatus::Executed));

        context.insert("temperature".to_string(), serde_json::json!(68));
        let result = evaluate_rule(&rule, &context, Utc::now());
        assert!(!result.fired);
        assert!(matches!(result.actions[0].status, ActionStatus::Skipped));
    }

    #[test]
    fn schedule_state_advances_deterministically() {
        let trigger = Trigger::Interval { seconds: 5 };
        let mut schedule = ScheduleState::new(&trigger, 0);
        let trigger_tick = ((5 * 1000 + TICK_INTERVAL_MS - 1) / TICK_INTERVAL_MS).max(1);
        assert!(schedule.should_fire(trigger_tick));
        let next_before = schedule.next_tick;
        schedule.advance();
        assert!(schedule.next_tick > next_before);
    }
}
