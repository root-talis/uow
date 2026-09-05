use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

use dotenv::{EnvLoader, EnvMap, EnvSequence};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use color_eyre::eyre::{Result, WrapErr};

mod application;
mod container;
mod infrastructure;
mod api;

use crate::container::Container;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with an env-filter.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "balance_transfer=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let env = EnvLoader::with_paths(["examples/balance-transfer/.env", ".env"])
        .required(false)
        .sequence(EnvSequence::InputThenEnv)
        .load()?;

    let addr = get_listen_addr(&env);
    let pool = setup_sqlite(&env).await?;

    let container = Container::new(pool);

    // Create the router with request tracing.
    let app = api::foo_router(container).layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = TcpListener::bind(&addr).await.expect("Failed to bind TCP listener");
    info!("Server listening on http://{}", addr);

    // Serve with graceful shutdown.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .wrap_err("server failed")
}

fn get_listen_addr(env: &EnvMap) -> String {
    let port: u16 = env.var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let addr: IpAddr = env.var("ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
    ;

    format!("{}:{}", addr, port)
}

async fn setup_sqlite(env: &EnvMap) -> Result<SqlitePool> {
    let connection_string = env.var("DATABASE_URL")
        .ok()
        .unwrap_or_else(||String::from("sqlite://dev.db"));

    let opts = SqliteConnectOptions::from_str(connection_string.as_str())?
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(opts).await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    info!("Migrations applied");

    Ok(pool)
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
