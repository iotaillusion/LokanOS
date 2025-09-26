use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use common_config::service_port;
use common_obs::{
    encode_prometheus_metrics, handler_latency_seconds, health_router, http_requests_total,
    ObsInit, PROMETHEUS_CONTENT_TYPE,
};
use tokio::net::TcpListener;

use std::time::Instant;

const SERVICE_NAME: &str = "telemetry-pipe";
const PORT_ENV: &str = "TELEMETRY_PIPE_PORT";
const DEFAULT_PORT: u16 = 8007;

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

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
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
        .route("/metrics", get(metrics))
        .merge(health_router(SERVICE_NAME))
        .layer(from_fn(track_http_metrics));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

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
