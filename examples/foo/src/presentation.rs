use axum::extract::Query;
use axum::{Router, routing};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::container::Container;

pub(crate) fn foo_router(container: Container) -> Router {
    Router::new()
        .route("/api/transfer", routing::post(transfer_handler))
        .route("/api/balance", routing::get(get_balance_handler))
        .with_state(container)
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

//
// Transfer
//

#[derive(Deserialize)]
pub struct TransferRequest {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: u32,
}

async fn transfer_handler(
    State(container): State<Container>,
    Json(payload): Json<TransferRequest>,
) -> impl IntoResponse {
    let req = super::application::TransferRequest {
        from_user_id: payload.from_user_id,
        to_user_id:   payload.to_user_id,
        amount:       payload.amount,
    };

    if let Err(e) = container.transfer_usecase.execute(req).await {
        error!(e = ?e, "{}", e.to_string());
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            error: e.to_string(),
        })));
    }

    Ok(StatusCode::OK)
}

//
// Get balance
//

#[derive(Deserialize)]
pub struct GetBalanceRequest {
    pub user_id: i64,
}

#[derive(Serialize)]
pub struct GetBalanceResponse {
    pub balance: u32,
}

async fn get_balance_handler(
    State(container): State<Container>,
    Query(payload): Query<GetBalanceRequest>,
) -> impl IntoResponse {
    let req = super::application::GetBalanceRequest {
        user_id: payload.user_id,
    };

    match container.get_balance_usecase.execute(req).await {
        Ok(balance) => 
            Ok((StatusCode::OK, Json(GetBalanceResponse {
                balance
            }))),

        Err(e) => {
            error!(e = ?e, "{}", e.to_string());
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: e.to_string(),
            })))
        }
    }
}