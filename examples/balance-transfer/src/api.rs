use std::num::NonZeroU32;

use axum::{Router, routing};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::container::Container;
use crate::domain::value_objects::{AccountNumber, CreditsAmount};

pub(crate) fn create_router() -> Router<Container> {
    Router::new()
        .route("/balance",  routing::post(balance_handler))
        .route("/transfer", routing::post(transfer_handler))
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

//
// Get balance
//

#[derive(Deserialize)]
pub struct GetBalanceRequest {
    pub account_number: NonZeroU32,
}

#[derive(Serialize)]
pub struct GetBalanceResponse {
    pub balance: u32,
}

async fn balance_handler(
    State(container): State<Container>,
    Json(payload): Json<GetBalanceRequest>,
) -> impl IntoResponse {
    let account_number = AccountNumber::new(payload.account_number);

    match container.get_balance_usecase.execute(account_number).await {
        Ok(balance) => 
            Ok((StatusCode::OK, Json(GetBalanceResponse {
                balance: *balance.inner()
            }))),

        Err(e) => {
            error!(e = ?e, "{}", e.to_string());
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: e.to_string(),
            })))
        }
    }
}

//
// Transfer
//

#[derive(Deserialize)]
pub struct TransferRequest {
    pub from: NonZeroU32,
    pub to: NonZeroU32,
    pub amount: u32,
}

async fn transfer_handler(
    State(container): State<Container>,
    Json(payload): Json<TransferRequest>,
) -> impl IntoResponse {
    let req = super::application::usecases::TransferRequest {
        from:   AccountNumber::new(payload.from),
        to:     AccountNumber::new(payload.to),
        amount: CreditsAmount::new(payload.amount),
    };

    if let Err(e) = container.transfer_usecase.execute(req).await {
        error!(e = ?e, "{}", e.to_string());
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            error: e.to_string(),
        })));
    }

    Ok(StatusCode::OK)
}
