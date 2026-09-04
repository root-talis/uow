use axum::{routing, Router};

use crate::di::Container;

mod handlers;
mod errors;

/// Creates the API router wired to the given container.
pub fn create_router(container: Container) -> Router {
    Router::new()
        .route("/api/messages", routing::post(handlers::create_message).get(handlers::list_messages))
        .with_state(container)
}

pub mod foo {
    use axum::{Router, routing};
    use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
    use serde::Deserialize;
    use sqlx::{Sqlite, SqlitePool, Transaction};

    pub fn foo_router(fac: UoWFac) -> Router {
        Router::new()
            .route("/foo", routing::post(transfer_handler))
            .with_state(fac)
    }

    #[derive(Deserialize)]
    struct TransferRequest {
        from_user_id: i64,
        to_user_id: i64,
        amount: i64,
    }

    // 1. The Handler: Manages the transaction lifecycle and HTTP responses
    async fn transfer_handler(
        State(fac): State<UoWFac>,
        Json(payload): Json<TransferRequest>,
    ) -> impl IntoResponse {
        // Start the transaction
        let mut uow = match fac.create().await {
            Ok(uow) => uow,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
        };

        // Call the use case function, passing the transaction as a mutable reference
        if transfer_usecase(&mut uow, &payload).await.is_err() {
            // Transaction is automatically dropped and rolled back here on failure
            return StatusCode::BAD_REQUEST;
        }

        // Commit if the usecase succeeds
        match uow.commit().await {
            Ok(_) => StatusCode::OK,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[derive(Clone)]
    pub struct UoWFac {
        pool: SqlitePool,
    }
    impl UoWFac {
        pub fn new(pool: SqlitePool) -> Self {
            Self { pool }
        }

        pub async fn create(&self) -> Result<UoW<'_>, sqlx::Error> {
            let tx = self.pool.begin().await?;
            Ok(UoW { tx } )
        }
    }

    pub struct UoW<'a> {
        tx: Transaction<'a, Sqlite>,
    }

    impl <'a> UoW<'a> {
        pub async fn commit(self) -> Result<(), sqlx::Error> {
            self.tx.commit().await
        }

        pub fn tx_mut(&mut self) -> &mut Transaction<'a, Sqlite> {
            &mut self.tx
        }
    }

    // 2. The Use Case: Handles the business logic and queries
    async fn transfer_usecase(
        uow: &mut UoW<'_>,
        payload: &TransferRequest,
    ) -> Result<(), sqlx::Error> {
        // Perform the first query using the passed transaction reference
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(payload.amount)
            .bind(payload.from_user_id)
            .execute(&mut **uow.tx_mut()) // Re-borrow the transaction connection
            .await?;

        // Perform the second query
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(payload.amount)
            .bind(payload.to_user_id)
            .execute(&mut **uow.tx_mut())
            .await?;

        Ok(())
    }

}
