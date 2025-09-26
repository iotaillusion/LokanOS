use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::health::{HealthCheckError, HealthClient};
use crate::state::{CommitError, RollbackError, Slot, StageError, UpdaterState};
use crate::store::{StateStore, StateStoreError};

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
    #[error(transparent)]
    Stage(#[from] StageError),
    #[error(transparent)]
    Commit(#[from] CommitError),
    #[error(transparent)]
    Rollback(#[from] RollbackError),
    #[error(transparent)]
    Store(#[from] StateStoreError),
    #[error(transparent)]
    Health(#[from] HealthCheckError),
    #[error("health check quorum not satisfied before deadline")]
    HealthQuorumFailed,
}

#[derive(Clone)]
pub struct UpdaterCore {
    state: Arc<Mutex<UpdaterState>>,
    store: Arc<dyn StateStore>,
    health_client: Arc<dyn HealthClient>,
    health_endpoints: Arc<Vec<String>>,
    health_deadline: Duration,
    health_quorum: usize,
}

impl UpdaterCore {
    pub async fn new(
        store: Arc<dyn StateStore>,
        health_client: Arc<dyn HealthClient>,
        health_endpoints: Vec<String>,
        health_deadline: Duration,
        health_quorum: usize,
    ) -> Result<Self, UpdaterError> {
        let state = match store.load().await? {
            Some(state) => state,
            None => UpdaterState::default(),
        };

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            store,
            health_client,
            health_endpoints: Arc::new(health_endpoints),
            health_deadline,
            health_quorum,
        })
    }

    pub async fn state(&self) -> UpdaterState {
        self.state.lock().await.clone()
    }

    pub async fn stage(&self, artifact: String) -> Result<Slot, UpdaterError> {
        let mut state = self.state.lock().await;
        let slot = state.stage(artifact)?;
        self.store.save(&state).await?;
        Ok(slot)
    }

    pub async fn commit_on_health(&self) -> Result<Slot, UpdaterError> {
        let slot = {
            let mut state = self.state.lock().await;
            let slot = state.begin_commit()?;
            self.store.save(&state).await?;
            slot
        };

        let healthy = self
            .health_client
            .wait_for_quorum(
                self.health_endpoints.as_ref(),
                self.health_deadline,
                self.health_quorum,
            )
            .await?;

        let mut state = self.state.lock().await;
        if healthy {
            state.finalize_commit(slot);
            self.store.save(&state).await?;
            Ok(slot)
        } else {
            state.fail_commit(slot);
            self.store.save(&state).await?;
            Err(UpdaterError::HealthQuorumFailed)
        }
    }

    pub async fn mark_bad(&self) -> Result<Option<Slot>, UpdaterError> {
        let mut state = self.state.lock().await;
        let result = state.mark_active_bad();
        self.store.save(&state).await?;
        Ok(result)
    }

    pub async fn rollback(&self) -> Result<Slot, UpdaterError> {
        let mut state = self.state.lock().await;
        let slot = state.rollback()?;
        self.store.save(&state).await?;
        Ok(slot)
    }
}
