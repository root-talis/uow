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
        // Call the use case function, passing the transaction as a mutable reference
        if transfer_usecase(&fac, &payload).await.is_err() {
            // Transaction is automatically dropped and rolled back here on failure
            return StatusCode::BAD_REQUEST;
        }

        StatusCode::OK
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
        uow_factory: &UoWFac,
        payload: &TransferRequest,
    ) -> Result<(), sqlx::Error> {
        let mut uow = uow_factory.create().await?;

        infra_transfer_withdraw(&mut uow, payload.from_user_id, payload.amount).await?;
        infra_transfer_deposit(&mut uow, payload.to_user_id, payload.amount).await?;
        
        uow.commit().await?;

        Ok(())
    }

    async fn infra_transfer_withdraw(
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut()) // Re-borrow the transaction connection
            .await?;

        Ok(())
    }

    async fn infra_transfer_deposit(
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut()) // Re-borrow the transaction connection
            .await?;

        Ok(())
    }

}
