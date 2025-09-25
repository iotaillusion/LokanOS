//! Shared configuration helpers for LokanOS services.

use std::env;

/// Resolve the port for a service from an environment variable.
///
/// Falls back to the provided default when the variable is missing or cannot be
/// parsed into a `u16`.
pub fn service_port(var: &str, default: u16) -> u16 {
    match env::var(var) {
        Ok(value) => value
            .parse::<u16>()
            .inspect_err(|error| {
                tracing::warn!(%var, %value, %error, "invalid port override, using default");
            })
            .unwrap_or(default),
        Err(_) => default,
    }
}
