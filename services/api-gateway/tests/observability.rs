use std::path::Path;
use std::sync::Arc;

use api_gateway::audit::AuditClient;
use api_gateway::config::RateLimitSettings;
use api_gateway::device_registry::DeviceRegistryClient;
use api_gateway::rate_limit::RateLimiter;
use api_gateway::rbac::RbacPolicy;
use api_gateway::{build_router, AppState};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common_msgbus::{BusMessage, MessageBus, MsgBusError, Subscription};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[derive(Clone, Default)]
struct NullBus;

#[async_trait]
impl MessageBus for NullBus {
    async fn publish(&self, _subject: &str, _payload: &[u8]) -> Result<(), MsgBusError> {
        Ok(())
    }

    async fn subscribe(&self, _subject: &str) -> Result<Subscription, MsgBusError> {
        Err(MsgBusError::Subscribe("not implemented".into()))
    }

    async fn request(&self, _subject: &str, _payload: &[u8]) -> Result<BusMessage, MsgBusError> {
        Err(MsgBusError::Request("not implemented".into()))
    }

    async fn respond(&self, _reply_to: &str, _payload: &[u8]) -> Result<(), MsgBusError> {
        Err(MsgBusError::Publish("not implemented".into()))
    }
}

#[tokio::test]
async fn metrics_endpoint_returns_uptime() {
    let policy_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/rbac.yaml");
    let policy = Arc::new(RbacPolicy::from_path(&policy_path).expect("policy"));

    let audit = AuditClient::new(String::new(), false);
    let rate_limiter = RateLimiter::new(&RateLimitSettings {
        requests_per_minute: 500,
        burst: 100,
    });
    let device_client =
        DeviceRegistryClient::new("http://127.0.0.1:8001".to_string()).expect("device client");

    let bus: Arc<dyn MessageBus> = Arc::new(NullBus::default());

    let state = Arc::new(AppState {
        policy,
        audit,
        rate_limiter,
        device_client,
        bus,
    });

    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("metrics response");

    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .expect("content-type header");
    assert_eq!(content_type, "text/plain; version=0.0.4");

    let body_bytes = body.collect().await.unwrap().to_bytes();
    let body_text = String::from_utf8(body_bytes.to_vec()).expect("utf8");
    assert!(body_text.contains("process_uptime_seconds"));
}
