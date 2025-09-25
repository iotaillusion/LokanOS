//! BLE commissioning shim definitions used across platform services.

use serde::{Deserialize, Serialize};

/// Request issued by a device to generate a certificate signing request (CSR).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CsrRequest {
    /// Globally unique identifier for the requesting device.
    pub device_id: String,
    /// Binary CSR payload (DER encoded) represented as base64 in transit.
    pub csr: String,
    /// Optional nonce to bind the CSR to a commissioning session.
    #[serde(default)]
    pub nonce: Option<String>,
}

impl Default for CsrRequest {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            csr: String::new(),
            nonce: None,
        }
    }
}

/// Response returned after the CSR is processed by the platform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CsrResponse {
    /// Base64-encoded device certificate.
    pub certificate: String,
    /// Optional trust anchor identifier that signed the certificate.
    #[serde(default)]
    pub ca_identifier: Option<String>,
}

impl Default for CsrResponse {
    fn default() -> Self {
        Self {
            certificate: String::new(),
            ca_identifier: None,
        }
    }
}

/// Verification payload sent once the device applied the commissioned credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    /// Device identifier performing verification.
    pub device_id: String,
    /// Signature covering the attestation challenge.
    pub signature: String,
    /// Optional opaque session identifier.
    #[serde(default)]
    pub session: Option<String>,
}

impl Default for VerifyRequest {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            signature: String::new(),
            session: None,
        }
    }
}

/// Verification response returned to the device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    /// Whether the verification succeeded.
    pub accepted: bool,
    /// Optional textual reason for rejection.
    #[serde(default)]
    pub reason: Option<String>,
}

impl Default for VerifyResponse {
    fn default() -> Self {
        Self {
            accepted: false,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serialization() {
        let request = CsrRequest {
            device_id: "device-123".into(),
            csr: "YmFzZTY0IGNzciBieXRlcw==".into(),
            nonce: Some("abc123".into()),
        };
        let json = serde_json::to_string(&request).expect("serialize");
        assert!(json.contains("deviceId"));
        let decoded: CsrRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(request, decoded);
    }
}
