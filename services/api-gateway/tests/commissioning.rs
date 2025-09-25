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
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use common_msgbus::{BusMessage, MessageBus, MsgBusError, Subscription};
use http_body_util::BodyExt;
use serde_json::json;
use tokio::sync::Mutex;
use tower::ServiceExt;

#[derive(Clone, Default)]
struct MockBus {
    events: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

#[async_trait]
impl MessageBus for MockBus {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), MsgBusError> {
        self.events
            .lock()
            .await
            .push((subject.to_string(), payload.to_vec()));
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
async fn commissioning_flow_emits_bus_events() {
    let policy_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/rbac.yaml");
    let policy = Arc::new(RbacPolicy::from_path(&policy_path).expect("policy"));

    let audit = AuditClient::new(String::new(), false);
    let rate_limiter = RateLimiter::new(&RateLimitSettings {
        requests_per_minute: 500,
        burst: 100,
    });
    let device_client =
        DeviceRegistryClient::new("http://127.0.0.1:8001".to_string()).expect("device client");

    let mock_bus = MockBus::default();
    let events = mock_bus.events.clone();
    let bus: Arc<dyn MessageBus> = Arc::new(mock_bus);

    let state = Arc::new(AppState {
        policy,
        audit,
        rate_limiter,
        device_client,
        bus,
    });

    let router = build_router(state);

    let handshake_payload = json!({
        "qrPayload": "LOKAN:thread-demo",
        "deviceId": "device-001",
        "nonce": "abc123",
    });

    let handshake_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/commissioning/ble/handshake")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-lokan-role", "guest")
                .header("x-lokan-subject", "commissioner")
                .body(Body::from(handshake_payload.to_string()))
                .unwrap(),
        )
        .await
        .expect("handshake response");

    assert_eq!(handshake_response.status(), StatusCode::OK);
    let handshake_body = handshake_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let handshake_json: serde_json::Value = serde_json::from_slice(&handshake_body).unwrap();
    let session = handshake_json
        .get("session")
        .and_then(|value| value.as_str())
        .expect("session")
        .to_string();
    assert!(handshake_json.get("sharedKey").is_some());

    let csr_payload = BASE64.encode(b"fake-csr-payload");
    let csr_request = json!({
        "deviceId": "device-001",
        "csr": csr_payload,
        "nonce": session,
    });

    let csr_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/commissioning/csr")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-lokan-role", "guest")
                .header("x-lokan-subject", "commissioner")
                .body(Body::from(csr_request.to_string()))
                .unwrap(),
        )
        .await
        .expect("csr response");

    assert_eq!(csr_response.status(), StatusCode::OK);
    let csr_body = csr_response.into_body().collect().await.unwrap().to_bytes();
    let csr_json: serde_json::Value = serde_json::from_slice(&csr_body).unwrap();
    assert!(csr_json.get("certificate").is_some());

    let verify_payload = json!({
        "deviceId": "device-001",
        "signature": BASE64.encode(vec![0x42; 32]),
        "session": session,
    });

    let verify_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/commissioning/verify")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-lokan-role", "guest")
                .header("x-lokan-subject", "commissioner")
                .body(Body::from(verify_payload.to_string()))
                .unwrap(),
        )
        .await
        .expect("verify response");

    assert_eq!(verify_response.status(), StatusCode::OK);

    let recorded = events.lock().await;
    assert_eq!(recorded.len(), 3);

    let handshake_event: serde_json::Value = serde_json::from_slice(&recorded[0].1).unwrap();
    assert_eq!(recorded[0].0, "radio.commissioning.handshake");
    assert_eq!(handshake_event["deviceId"], "device-001");

    let csr_event: serde_json::Value = serde_json::from_slice(&recorded[1].1).unwrap();
    assert_eq!(recorded[1].0, "radio.commissioning.csr");
    assert_eq!(csr_event["csrLength"].as_u64().unwrap(), 16);

    let verify_event: serde_json::Value = serde_json::from_slice(&recorded[2].1).unwrap();
    assert_eq!(recorded[2].0, "radio.commissioning.verify");
    assert_eq!(verify_event["signatureLength"].as_u64().unwrap(), 32);
}
