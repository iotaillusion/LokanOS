use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::state::UpdaterState;

#[derive(Debug, thiserror::Error)]
pub enum StateStoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse updater state: {0}")]
    Parse(#[from] serde_json::Error),
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn load(&self) -> Result<Option<UpdaterState>, StateStoreError>;
    async fn save(&self, state: &UpdaterState) -> Result<(), StateStoreError>;
}

#[derive(Debug, Clone)]
pub struct FileStateStore {
    path: PathBuf,
}

impl FileStateStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    async fn ensure_parent_dir(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl StateStore for FileStateStore {
    async fn load(&self) -> Result<Option<UpdaterState>, StateStoreError> {
        match fs::read(&self.path).await {
            Ok(bytes) => {
                let state = serde_json::from_slice(&bytes)?;
                Ok(Some(state))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StateStoreError::Io(err)),
        }
    }

    async fn save(&self, state: &UpdaterState) -> Result<(), StateStoreError> {
        self.ensure_parent_dir().await?;
        let json = serde_json::to_vec_pretty(state)?;
        let mut file = fs::File::create(&self.path).await?;
        file.write_all(&json).await?;
        file.flush().await?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemoryStateStore {
    pub state: tokio::sync::Mutex<Option<UpdaterState>>,
}

#[async_trait]
impl StateStore for MemoryStateStore {
    async fn load(&self) -> Result<Option<UpdaterState>, StateStoreError> {
        Ok(self.state.lock().await.clone())
    }

    async fn save(&self, state: &UpdaterState) -> Result<(), StateStoreError> {
        *self.state.lock().await = Some(state.clone());
        Ok(())
    }
}
