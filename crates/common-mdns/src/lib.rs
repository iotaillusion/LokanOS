//! Minimal mDNS announcer stub used during early development.

use thiserror::Error;

/// Errors returned by the announcer stub.
#[derive(Debug, Error)]
pub enum MdnsError {
    /// Placeholder error for when an announcement cannot be created.
    #[error("failed to announce service over mDNS: {0}")]
    Announcement(String),
}

/// Handle returned when announcing a service.
#[derive(Debug, Clone)]
pub struct Announcement {
    service: String,
    port: u16,
}

impl Announcement {
    /// Service label that was advertised.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// TCP port that was advertised.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop advertising the service.
    pub async fn shutdown(self) -> Result<(), MdnsError> {
        tracing::info!(service = %self.service, port = self.port, "stopping mDNS announcement (stub)");
        Ok(())
    }
}

/// Announce a TCP service via mDNS.
///
/// This is currently a stub implementation that only emits structured logs.
pub async fn announce(service: &str, port: u16) -> Result<Announcement, MdnsError> {
    if service.is_empty() {
        return Err(MdnsError::Announcement(
            "service name must not be empty".into(),
        ));
    }

    tracing::info!(service, port, "starting mDNS announcement (stub)");
    Ok(Announcement {
        service: service.to_string(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn announce_returns_handle() {
        let handle = announce("_lokan._tcp", 1234).await.expect("announcement");
        assert_eq!(handle.service(), "_lokan._tcp");
        assert_eq!(handle.port(), 1234);
        handle.shutdown().await.expect("shutdown");
    }
}
