use axum::{Router, routing};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use tracing::error;

use crate::container::Container;

pub(crate) fn foo_router(container: Container) -> Router {
    Router::new()
        .route("/api/transfer", routing::post(transfer_handler))
        .with_state(container)
}

#[derive(Deserialize)]
pub struct TransferRequest {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: u32,
}

impl From<TransferRequest> for super::application::TransferRequest {
    fn from(value: TransferRequest) -> Self {
        Self {
            from_user_id: value.from_user_id,
            to_user_id:   value.to_user_id,
            amount:       value.amount,
        }
    }
}

async fn transfer_handler(
    State(container): State<Container>,
    Json(payload): Json<TransferRequest>,
) -> impl IntoResponse {
    if let Err(e) = container.transfer_usecase.execute(payload.into()).await {
        error!(e = e.to_string(), "failed to transfer funds");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    StatusCode::OK
}
