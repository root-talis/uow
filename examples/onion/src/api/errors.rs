use axum::{Json, http::StatusCode, response::IntoResponse};
use tracing::error;

/// Application error type for HTTP handlers.
pub enum AppError {
    UseCase(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            AppError::UseCase(msg) => {
                error!("Application error: {}", msg);
                let body = serde_json::json!({ "error": msg });
                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
        }
    }
}
