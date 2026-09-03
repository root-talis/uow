use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::errors::AppError;

use crate::application::dto::Message as MessageDto;
use crate::application::usecase::SendEmailRequest;
use crate::di::Container;

/// POST /api/messages — sends a new message.
pub async fn create_message(
    State(container): State<Container>,
    Json(req): Json<CreateMessageRequest>,
) -> Result<impl IntoResponse, AppError> {
    container
        .send_email
        .execute(SendEmailRequest {
            message: req.message,
        })
        .await
        .map_err(AppError::UseCase)?;

    Ok(StatusCode::CREATED)
}

/// GET /api/messages — dumps all messages.
pub async fn list_messages(
    State(container): State<Container>,
) -> Result<Json<Vec<MessageResponse>>, AppError> {
    let messages = container.dump_emails.execute().await.map_err(AppError::UseCase)?;

    let responses: Vec<MessageResponse> = messages.into_iter().map(Into::into).collect();

    Ok(Json(responses))
}


/// JSON request body for sending a message.
#[derive(Deserialize)]
pub struct CreateMessageRequest {
    pub message: String,
}

/// JSON response representing a message.
#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
    pub created_at: DateTime<Utc>,
}

impl From<MessageDto> for MessageResponse {
    fn from(msg: MessageDto) -> Self {
        Self {
            message: msg.message,
            created_at: msg.created_at,
        }
    }
}
