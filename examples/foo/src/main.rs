use std::env;
use std::net::{IpAddr, Ipv4Addr};

use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use color_eyre::eyre::{Result, WrapErr};

mod application;
mod container;
mod infrastructure;
mod presentation;
mod uow;
mod uow_sqlx;

use crate::container::Container;

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
    let app = presentation::foo_router(container).layer(tower_http::trace::TraceLayer::new_for_http());

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
