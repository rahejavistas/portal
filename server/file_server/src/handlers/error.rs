//! Shared error-response helpers: a JSON string body paired with a status code.

use axum::{http::StatusCode, Json};

pub type ApiError = (StatusCode, Json<String>);

/// Client-facing message for internal errors — never the raw error text.
const GENERIC_INTERNAL_MESSAGE: &str = "An internal error occurred. Please try again.";

pub fn internal_err(e: impl std::fmt::Display) -> ApiError {
    eprintln!("Internal error: {}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(GENERIC_INTERNAL_MESSAGE.to_string()),
    )
}

pub fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

pub fn forbidden(msg: impl Into<String>) -> ApiError {
    (StatusCode::FORBIDDEN, Json(msg.into()))
}

pub fn too_many_requests(msg: impl Into<String>) -> ApiError {
    (StatusCode::TOO_MANY_REQUESTS, Json(msg.into()))
}

/// Shorthand for the common "DB/internal error -> 500" mapping.
pub trait ResultExt<T> {
    fn internal_err(self) -> Result<T, ApiError>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    fn internal_err(self) -> Result<T, ApiError> {
        self.map_err(|e| {
            eprintln!("Internal error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GENERIC_INTERNAL_MESSAGE.to_string()),
            )
        })
    }
}
