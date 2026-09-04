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
