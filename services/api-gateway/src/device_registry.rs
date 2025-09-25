use crate::error::ApiError;
use reqwest::Url;
use serde_json::Value;

#[derive(Clone)]
pub struct DeviceRegistryClient {
    base_url: Url,
    client: reqwest::Client,
}

impl DeviceRegistryClient {
    pub fn new(base_url: String) -> Result<Self, ApiError> {
        let base_url = Url::parse(&base_url).map_err(|_| ApiError::Internal)?;
        Ok(Self {
            base_url,
            client: reqwest::Client::new(),
        })
    }

    pub async fn list_devices(&self) -> Result<Value, ApiError> {
        let url = self
            .base_url
            .join("/v1/devices")
            .map_err(|_| ApiError::Internal)?;
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::Upstream(format!(
                "device registry responded with status {}",
                status
            )));
        }
        response.json::<Value>().await.map_err(ApiError::from)
    }
}
