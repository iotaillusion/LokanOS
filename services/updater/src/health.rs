use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tokio::time::sleep;

#[derive(Debug, thiserror::Error)]
pub enum HealthCheckError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

#[async_trait]
pub trait HealthClient: Send + Sync {
    async fn wait_for_quorum(
        &self,
        endpoints: &[String],
        deadline: Duration,
        quorum: usize,
    ) -> Result<bool, HealthCheckError>;
}

#[derive(Debug, Clone)]
pub struct HttpHealthClient {
    client: Client,
    poll_interval: Duration,
}

impl Default for HttpHealthClient {
    fn default() -> Self {
        Self::new(Duration::from_millis(250))
    }
}

impl HttpHealthClient {
    pub fn new(poll_interval: Duration) -> Self {
        Self {
            client: Client::builder().build().expect("reqwest client"),
            poll_interval,
        }
    }
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

#[async_trait]
impl HealthClient for HttpHealthClient {
    async fn wait_for_quorum(
        &self,
        endpoints: &[String],
        deadline: Duration,
        quorum: usize,
    ) -> Result<bool, HealthCheckError> {
        if quorum == 0 || endpoints.is_empty() {
            return Ok(true);
        }

        let quorum = quorum.min(endpoints.len());
        let deadline_at = Instant::now() + deadline;

        loop {
            let mut healthy = 0;
            for endpoint in endpoints {
                let response = self.client.get(endpoint).send().await?.error_for_status()?;

                if response.status().is_success() {
                    match response.json::<HealthResponse>().await {
                        Ok(body) if body.status.eq_ignore_ascii_case("ok") => healthy += 1,
                        _ => {}
                    }
                }
            }

            if healthy >= quorum {
                return Ok(true);
            }

            if Instant::now() >= deadline_at {
                return Ok(false);
            }

            sleep(self.poll_interval).await;
        }
    }
}

#[derive(Debug, Default)]
pub struct StubHealthClient {
    pub result: bool,
}

#[async_trait]
impl HealthClient for StubHealthClient {
    async fn wait_for_quorum(
        &self,
        _endpoints: &[String],
        _deadline: Duration,
        _quorum: usize,
    ) -> Result<bool, HealthCheckError> {
        Ok(self.result)
    }
}
