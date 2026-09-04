use std::env;
use std::net::{IpAddr, Ipv4Addr};

use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use color_eyre::eyre::{Result, WrapErr};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with an env-filter.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "foo=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Read port from env, default to 3000.
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    // Read addr from env, default to 0.0.0.0
    let addr: IpAddr = env::var("ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
    ;

    let addr = format!("{}:{}", addr, port);

    let pool = SqlitePool::connect("sqlite::memory:").await.wrap_err("failed to create sqlite pool")?;

    // Bootstrap the dependency container.
    let container = Container::new(pool);

    // Create the router with request tracing.
    let app = foo::foo_router(container).layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = TcpListener::bind(&addr).await.expect("Failed to bind TCP listener");
    info!("Server listening on http://{}", addr);

    // Serve with graceful shutdown.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .wrap_err("server failed")
}

/// Waits for a shutdown signal (Ctrl+C).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            warn!("Received Ctrl+C, shutting down gracefully");
        }
        _ = terminate => {
            warn!("Received SIGTERM, shutting down gracefully");
        }
    }
}

#[derive(Clone)]
struct Container {
    uow_fac: foo::UoWFac,
}

impl Container {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            uow_fac: foo::UoWFac::new(pool)
        }
    }
}


pub mod foo {
    use axum::{Router, routing};
    use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
    use serde::Deserialize;
    use sqlx::{Sqlite, SqlitePool, Transaction};
    use tracing::error;

    //
    // Presentation layer
    //

    pub(crate) fn foo_router(container: super::Container) -> Router {
        Router::new()
            .route("/foo", routing::post(transfer_handler))
            .with_state(container)
    }

    #[derive(Deserialize)]
    struct TransferRequest {
        from_user_id: i64,
        to_user_id: i64,
        amount: i64,
    }

    // 1. The Handler: Manages the transaction lifecycle and HTTP responses
    async fn transfer_handler(
        State(container): State<super::Container>,
        Json(payload): Json<TransferRequest>,
    ) -> impl IntoResponse {
        let fac = container.uow_fac;

        // Call the use case function, passing the transaction as a mutable reference
        if let Err(e) = transfer_usecase(&fac, &payload).await {
            error!(e = e.to_string(), "failed to transfer funds");
            // Transaction is automatically dropped and rolled back here on failure
            return StatusCode::INTERNAL_SERVER_ERROR;
        }

        StatusCode::OK
    }

    //
    // Application layer
    //

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

    //
    // Transaction management stuff
    //

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

    //
    // Infrastructure layer
    //

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
