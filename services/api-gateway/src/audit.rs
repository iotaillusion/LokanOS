use serde::Serialize;
use std::time::SystemTime;

#[derive(Clone)]
pub struct AuditClient {
    endpoint: Option<String>,
    client: reqwest::Client,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub actor: String,
    pub role: String,
    pub action: String,
    pub resource: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    pub timestamp: SystemTime,
}

impl AuditClient {
    pub fn new(endpoint: String, enabled: bool) -> Self {
        Self {
            endpoint: if enabled && !endpoint.is_empty() {
                Some(endpoint)
            } else {
                None
            },
            client: reqwest::Client::new(),
            enabled,
        }
    }

    pub async fn record(&self, event: AuditEvent) {
        if !self.enabled {
            tracing::trace!(action = %event.action, "audit disabled; dropping event");
            return;
        }

        if let Some(endpoint) = &self.endpoint {
            let request = self.client.post(endpoint).json(&event).send().await;
            if let Err(error) = request {
                tracing::warn!(%error, endpoint, "failed to deliver audit event");
            }
        } else {
            tracing::info!(action = %event.action, outcome = %event.outcome, resource = %event.resource, actor = %event.actor, "audit endpoint not configured; logging event");
        }
    }
}

impl AuditEvent {
    pub fn new(
        actor: String,
        role: String,
        action: String,
        resource: String,
        outcome: String,
    ) -> Self {
        Self {
            actor,
            role,
            action,
            resource,
            outcome,
            detail: None,
            timestamp: SystemTime::now(),
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn with_outcome(mut self, outcome: impl Into<String>) -> Self {
        self.outcome = outcome.into();
        self
    }
}
