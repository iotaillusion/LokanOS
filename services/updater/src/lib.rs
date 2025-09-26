use std::net::SocketAddr;
use std::time::Instant;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, health_router, http_request_observe, ObsInit, ObsInitError,
    PROMETHEUS_CONTENT_TYPE,
};
use tokio::net::TcpListener;

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
    let app = build_router();
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

pub fn build_router() -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .merge(health_router(SERVICE_NAME))
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
