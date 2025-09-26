use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use common_ble::{CsrRequest, CsrResponse, VerifyRequest, VerifyResponse};
use common_obs::msgbus_publish_total;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BleHandshakeRequest {
    pub qr_payload: String,
    pub device_id: String,
    pub nonce: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleHandshakeResponse {
    pub session: String,
    pub shared_key: String,
}

pub async fn ble_handshake(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BleHandshakeRequest>,
) -> Result<Json<BleHandshakeResponse>, ApiError> {
    validate_qr(&request.qr_payload)?;
    validate_device_id(&request.device_id)?;
    validate_nonce(&request.nonce)?;

    let session = Uuid::new_v4().to_string();
    let shared_key = generate_shared_secret();

    let event = json!({
        "type": "commissioning.handshake",
        "deviceId": request.device_id,
        "nonce": request.nonce,
        "session": session,
        "metadata": request.metadata,
    });

    publish_event(&state, "radio.commissioning.handshake", &event).await;

    Ok(Json(BleHandshakeResponse {
        session,
        shared_key,
    }))
}

pub async fn submit_csr(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CsrRequest>,
) -> Result<Json<CsrResponse>, ApiError> {
    validate_device_id(&request.device_id)?;
    let csr_bytes = BASE64
        .decode(request.csr.as_bytes())
        .map_err(|_| ApiError::Validation {
            message: "csr must be valid base64 data".to_string(),
        })?;
    if csr_bytes.is_empty() {
        return Err(ApiError::Validation {
            message: "csr payload cannot be empty".to_string(),
        });
    }

    if let Some(nonce) = &request.nonce {
        validate_nonce(nonce)?;
    }

    let mut certificate_bytes = Vec::new();
    certificate_bytes.extend_from_slice(b"lokan-dev-cert:");
    certificate_bytes.extend_from_slice(request.device_id.as_bytes());
    certificate_bytes.extend_from_slice(b":");
    certificate_bytes.extend_from_slice(&csr_bytes[..csr_bytes.len().min(8)]);

    let certificate = BASE64.encode(certificate_bytes);
    let response = CsrResponse {
        certificate,
        ca_identifier: Some("lokan-dev-root-ca".to_string()),
    };

    let event = json!({
        "type": "commissioning.csr",
        "deviceId": request.device_id,
        "nonce": request.nonce,
        "csrLength": csr_bytes.len(),
    });
    publish_event(&state, "radio.commissioning.csr", &event).await;

    Ok(Json(response))
}

pub async fn verify_credentials(
    State(state): State<Arc<AppState>>,
    Json(request): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    validate_device_id(&request.device_id)?;
    let signature =
        BASE64
            .decode(request.signature.as_bytes())
            .map_err(|_| ApiError::Validation {
                message: "signature must be valid base64 data".to_string(),
            })?;
    if signature.len() < 16 {
        return Err(ApiError::Validation {
            message: "signature must be at least 16 bytes".to_string(),
        });
    }

    if let Some(session) = &request.session {
        validate_nonce(session)?;
    }

    let event = json!({
        "type": "commissioning.verify",
        "deviceId": request.device_id,
        "session": request.session,
        "signatureLength": signature.len(),
    });
    publish_event(&state, "radio.commissioning.verify", &event).await;

    Ok(Json(VerifyResponse {
        accepted: true,
        reason: None,
    }))
}

fn validate_qr(qr: &str) -> Result<(), ApiError> {
    if qr.is_empty() {
        return Err(ApiError::Validation {
            message: "qrPayload cannot be empty".to_string(),
        });
    }
    if !qr.starts_with("LOKAN:") {
        return Err(ApiError::Validation {
            message: "qrPayload must begin with the LOKAN: prefix".to_string(),
        });
    }
    Ok(())
}

fn validate_device_id(device_id: &str) -> Result<(), ApiError> {
    if device_id.len() < 4 || device_id.len() > 64 {
        return Err(ApiError::Validation {
            message: "deviceId must be between 4 and 64 characters".to_string(),
        });
    }
    if !device_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::Validation {
            message: "deviceId must be alphanumeric and may include '-' or '_'".to_string(),
        });
    }
    Ok(())
}

fn validate_nonce(nonce: &str) -> Result<(), ApiError> {
    if nonce.len() < 6 || nonce.len() > 128 {
        return Err(ApiError::Validation {
            message: "nonce must be between 6 and 128 characters".to_string(),
        });
    }
    if !nonce
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::Validation {
            message: "nonce must be alphanumeric and may include '-' or '_'".to_string(),
        });
    }
    Ok(())
}

fn generate_shared_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    BASE64.encode(bytes)
}

async fn publish_event(state: &AppState, subject: &str, payload: &serde_json::Value) {
    let Ok(bytes) = serde_json::to_vec(payload) else {
        warn!(subject, "failed to serialize commissioning event");
        return;
    };

    msgbus_publish_total().inc(&[crate::SERVICE_NAME, subject], 1);
    if let Err(error) = state.bus.publish(subject, &bytes).await {
        warn!(%error, subject, "failed to publish commissioning event");
    }
}
