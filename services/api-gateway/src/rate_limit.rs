use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::config::RateLimitSettings;
use crate::error::ApiError;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    capacity: f64,
    rate_per_second: f64,
}

struct State {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(settings: &RateLimitSettings) -> Self {
        let capacity = settings.burst.max(1) as f64;
        let rate_per_second = (settings.requests_per_minute.max(1) as f64) / 60.0;
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    tokens: capacity,
                    last_refill: Instant::now(),
                }),
                capacity,
                rate_per_second,
            }),
        }
    }

    pub async fn check(&self) -> Result<(), ApiError> {
        let mut state = self.inner.state.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            let replenished = elapsed * self.inner.rate_per_second;
            state.tokens = (state.tokens + replenished).min(self.inner.capacity);
            state.last_refill = now;
        }

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            Ok(())
        } else {
            Err(ApiError::RateLimited {
                retry_after: Duration::from_secs(1),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_within_burst() {
        let limiter = RateLimiter::new(&RateLimitSettings {
            requests_per_minute: 120,
            burst: 2,
        });

        assert!(limiter.check().await.is_ok());
        assert!(limiter.check().await.is_ok());
    }

    #[tokio::test]
    async fn rejects_when_exhausted() {
        let limiter = RateLimiter::new(&RateLimitSettings {
            requests_per_minute: 2,
            burst: 1,
        });

        assert!(limiter.check().await.is_ok());
        let result = limiter.check().await;
        assert!(matches!(result, Err(ApiError::RateLimited { .. })));
    }

    #[tokio::test]
    async fn refills_over_time() {
        let limiter = RateLimiter::new(&RateLimitSettings {
            requests_per_minute: 60,
            burst: 1,
        });

        limiter.check().await.unwrap();
        assert!(limiter.check().await.is_err());
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(limiter.check().await.is_ok());
    }
}
