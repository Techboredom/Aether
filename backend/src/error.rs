use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Uniform error type for API handlers: maps Kubernetes/Postgres failures to a
/// sensible HTTP status and a `{"error": "..."}` JSON body.
pub enum ApiError {
    Kube(kube::Error),
    Sqlx(sqlx::Error),
    BadRequest(String),
    Unauthorized,
    Forbidden(String),
    /// The proxied app couldn't be reached (connection timeout/failure,
    /// handshake failure, ...) — distinct from `BadRequest` since it's not
    /// the caller's fault.
    ProxyUnavailable(String),
}

impl From<kube::Error> for ApiError {
    fn from(err: kube::Error) -> Self {
        ApiError::Kube(err)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Sqlx(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Kube(kube::Error::Api(status)) => (
                StatusCode::from_u16(status.code).unwrap_or(StatusCode::BAD_GATEWAY),
                status.message,
            ),
            ApiError::Kube(err) => (StatusCode::BAD_GATEWAY, err.to_string()),
            ApiError::Sqlx(err) => (StatusCode::SERVICE_UNAVAILABLE, err.to_string()),
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "not logged in".to_string()),
            ApiError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            ApiError::ProxyUnavailable(message) => (StatusCode::BAD_GATEWAY, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
