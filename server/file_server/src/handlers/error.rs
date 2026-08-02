//! Shared error-response helpers: a JSON string body paired with a status code.

use axum::{http::StatusCode, Json};

pub type ApiError = (StatusCode, Json<String>);

pub fn internal_err(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string()))
}

pub fn bad_request(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(msg.into()))
}

pub fn forbidden(msg: impl Into<String>) -> ApiError {
    (StatusCode::FORBIDDEN, Json(msg.into()))
}

/// Shorthand for the common "DB/internal error -> 500" mapping.
pub trait ResultExt<T> {
    fn internal_err(self) -> Result<T, ApiError>;
}

impl<T, E: ToString> ResultExt<T> for Result<T, E> {
    fn internal_err(self) -> Result<T, ApiError> {
        self.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))
    }
}
