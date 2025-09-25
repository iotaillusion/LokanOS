use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use common_config::service_port;
use common_obs::health_router;

const SERVICE_NAME: &str = "scene-svc";
const PORT_ENV: &str = "SCENE_SVC_PORT";
const DEFAULT_PORT: u16 = 8003;
const DEFAULT_REGISTRY_URL: &str = "http://127.0.0.1:8001";

#[derive(Clone)]
struct AppState<C: DeviceRegistryClient + Send + Sync + 'static> {
    executor: Arc<SceneExecutor<C>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SceneRequest {
    #[allow(dead_code)]
    pub scene_id: Option<String>,
    pub operations: Vec<DeviceOperation>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceOperation {
    pub device_id: String,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct SceneResponse {
    status: SceneStatus,
    results: Vec<DeviceApplyResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SceneStatus {
    Applied,
    PartialFailure,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
struct DeviceApplyResult {
    device_id: String,
    status: DeviceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum DeviceStatus {
    Applied,
    RolledBack,
    Failed,
    Skipped,
}

#[derive(Debug, thiserror::Error)]
enum SceneError {
    #[error("registry unreachable: {0}")]
    Registry(String),
    #[error("device not found: {0}")]
    Missing(String),
    #[error("unexpected response")]
    Unexpected,
}

#[async_trait]
trait DeviceRegistryClient: Clone + Send + Sync {
    async fn fetch_state(&self, device_id: &str) -> Result<serde_json::Value, SceneError>;
    async fn apply_state(
        &self,
        device_id: &str,
        state: &serde_json::Value,
    ) -> Result<(), SceneError>;
}

#[derive(Clone)]
struct HttpDeviceRegistry {
    client: reqwest::Client,
    base: String,
}

#[async_trait]
impl DeviceRegistryClient for HttpDeviceRegistry {
    async fn fetch_state(&self, device_id: &str) -> Result<serde_json::Value, SceneError> {
        let url = format!("{}/v1/devices/{}", self.base, device_id);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|err| SceneError::Registry(err.to_string()))?;
        if response.status().is_success() {
            let body: serde_json::Value = response
                .json()
                .await
                .map_err(|err| SceneError::Registry(err.to_string()))?;
            Ok(body
                .get("state")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})))
        } else if response.status().as_u16() == 404 {
            Err(SceneError::Missing(device_id.to_string()))
        } else {
            Err(SceneError::Unexpected)
        }
    }

    async fn apply_state(
        &self,
        device_id: &str,
        state: &serde_json::Value,
    ) -> Result<(), SceneError> {
        let url = format!("{}/v1/devices/{}/state", self.base, device_id);
        let response = self
            .client
            .put(url)
            .json(state)
            .send()
            .await
            .map_err(|err| SceneError::Registry(err.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else if response.status().as_u16() == 404 {
            Err(SceneError::Missing(device_id.to_string()))
        } else {
            Err(SceneError::Unexpected)
        }
    }
}

struct SceneExecutor<C: DeviceRegistryClient> {
    client: C,
}

impl<C: DeviceRegistryClient> SceneExecutor<C> {
    async fn apply_scene(&self, request: SceneRequest) -> SceneResponse {
        let mut results = Vec::with_capacity(request.operations.len());
        let mut previous_states: Vec<(String, serde_json::Value)> = Vec::new();
        let mut failure_encountered = false;

        for op in &request.operations {
            if failure_encountered {
                results.push(DeviceApplyResult {
                    device_id: op.device_id.clone(),
                    status: DeviceStatus::Skipped,
                    detail: Some("skipped due to prior failure".to_string()),
                });
                continue;
            }

            match self.client.fetch_state(&op.device_id).await {
                Ok(prev) => match self.client.apply_state(&op.device_id, &op.state).await {
                    Ok(_) => {
                        previous_states.push((op.device_id.clone(), prev));
                        results.push(DeviceApplyResult {
                            device_id: op.device_id.clone(),
                            status: DeviceStatus::Applied,
                            detail: None,
                        });
                    }
                    Err(err) => {
                        failure_encountered = true;
                        results.push(DeviceApplyResult {
                            device_id: op.device_id.clone(),
                            status: DeviceStatus::Failed,
                            detail: Some(err.to_string()),
                        });
                    }
                },
                Err(err) => {
                    failure_encountered = true;
                    results.push(DeviceApplyResult {
                        device_id: op.device_id.clone(),
                        status: DeviceStatus::Failed,
                        detail: Some(err.to_string()),
                    });
                }
            }
        }

        if failure_encountered {
            for (device_id, prev_state) in previous_states.into_iter().rev() {
                if let Err(err) = self.client.apply_state(&device_id, &prev_state).await {
                    results.push(DeviceApplyResult {
                        device_id,
                        status: DeviceStatus::Failed,
                        detail: Some(format!("rollback failed: {err}")),
                    });
                } else {
                    results.push(DeviceApplyResult {
                        device_id,
                        status: DeviceStatus::RolledBack,
                        detail: Some("rolled back".to_string()),
                    });
                }
            }
        }

        let status = if failure_encountered {
            if results
                .iter()
                .any(|r| matches!(r.status, DeviceStatus::RolledBack))
            {
                SceneStatus::PartialFailure
            } else {
                SceneStatus::Failed
            }
        } else {
            SceneStatus::Applied
        };

        SceneResponse { status, results }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port = service_port(PORT_ENV, DEFAULT_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let registry_url =
        std::env::var("DEVICE_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let client = HttpDeviceRegistry {
        client: reqwest::Client::new(),
        base: registry_url,
    };

    let state = AppState {
        executor: Arc::new(SceneExecutor { client }),
    };

    tracing::info!(%addr, service = SERVICE_NAME, "starting service");

    let app = Router::new()
        .route("/v1/scenes:apply", post(apply_scene))
        .with_state(state)
        .merge(health_router(SERVICE_NAME));

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn apply_scene<C: DeviceRegistryClient + Send + Sync + 'static>(
    State(state): State<AppState<C>>,
    Json(payload): Json<SceneRequest>,
) -> Json<SceneResponse> {
    let response = state.executor.apply_scene(payload).await;
    Json(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct MockRegistry {
        devices: Arc<Mutex<HashMap<String, serde_json::Value>>>,
        fail_on: Arc<Mutex<Option<String>>>,
    }

    #[async_trait]
    impl DeviceRegistryClient for MockRegistry {
        async fn fetch_state(&self, device_id: &str) -> Result<serde_json::Value, SceneError> {
            let devices = self.devices.lock().await;
            devices
                .get(device_id)
                .cloned()
                .ok_or_else(|| SceneError::Missing(device_id.to_string()))
        }

        async fn apply_state(
            &self,
            device_id: &str,
            state: &serde_json::Value,
        ) -> Result<(), SceneError> {
            if let Some(failing) = self.fail_on.lock().await.clone() {
                if failing == device_id {
                    return Err(SceneError::Registry("simulated failure".to_string()));
                }
            }
            let mut devices = self.devices.lock().await;
            devices.insert(device_id.to_string(), state.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn scene_rolls_back_on_failure() {
        let registry = MockRegistry::default();
        {
            let mut devices = registry.devices.lock().await;
            devices.insert("one".to_string(), serde_json::json!({"power": "off"}));
            devices.insert("two".to_string(), serde_json::json!({"power": "off"}));
        }
        *registry.fail_on.lock().await = Some("two".to_string());

        let executor = SceneExecutor {
            client: registry.clone(),
        };
        let response = executor
            .apply_scene(SceneRequest {
                scene_id: None,
                operations: vec![
                    DeviceOperation {
                        device_id: "one".to_string(),
                        state: serde_json::json!({"power": "on"}),
                    },
                    DeviceOperation {
                        device_id: "two".to_string(),
                        state: serde_json::json!({"power": "on"}),
                    },
                ],
            })
            .await;

        assert!(matches!(response.status, SceneStatus::PartialFailure));
        let applied = response
            .results
            .iter()
            .find(|r| r.device_id == "one" && matches!(r.status, DeviceStatus::RolledBack));
        assert!(applied.is_some(), "device one should have been rolled back");
    }
}
