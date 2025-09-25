use std::fmt;
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden { reason: String },
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("upstream call failed: {0}")]
    Upstream(String),
    #[error("internal server error")]
    Internal,
    #[error("invalid request: {message}")]
    Validation { message: String },
}

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    error: ErrorDetails<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorDetails<'a> {
    code: &'a str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, retry_after) = match &self {
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                self.to_string(),
                None,
            ),
            ApiError::Forbidden { reason } => {
                (StatusCode::FORBIDDEN, "forbidden", reason.clone(), None)
            }
            ApiError::RateLimited { retry_after } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                self.to_string(),
                Some(*retry_after),
            ),
            ApiError::Upstream(message) => (
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                message.clone(),
                None,
            ),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                self.to_string(),
                None,
            ),
            ApiError::Validation { message } => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                message.clone(),
                None,
            ),
        };

        let mut response = Json(ErrorBody {
            error: ErrorDetails { code, message },
        })
        .into_response();
        *response.status_mut() = status;

        if let Some(duration) = retry_after {
            if let Ok(header_value) = header_value_from_duration(duration) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, header_value);
            }
        }

        response
    }
}

fn header_value_from_duration(duration: Duration) -> Result<axum::http::HeaderValue, fmt::Error> {
    use std::fmt::Write;

    let mut buffer = String::new();
    write!(&mut buffer, "{}", duration.as_secs())?;
    axum::http::HeaderValue::from_str(&buffer).map_err(|_| fmt::Error)
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            ApiError::Upstream("request to upstream service timed out".to_string())
        } else if error.status().is_some() {
            ApiError::Upstream(error.to_string())
        } else {
            ApiError::Internal
        }
    }
}
