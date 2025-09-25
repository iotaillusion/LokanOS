//! Common authentication primitives shared across LokanOS services.

use thiserror::Error;

/// Represents a validation error for an authentication token.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The provided token failed basic validation rules.
    #[error("token validation failed: {0}")]
    Validation(String),
}

/// Verifies a raw authentication token.
///
/// This is a non-breaking stub that always accepts the token and should be
/// replaced with real validation logic in subsequent phases.
#[allow(unused_variables)]
pub fn validate_token(token: &str) -> Result<(), AuthError> {
    Ok(())
}
