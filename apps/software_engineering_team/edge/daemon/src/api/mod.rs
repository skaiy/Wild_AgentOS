pub mod chat;
pub mod health;
pub mod ws;

use std::sync::Arc;

use axum::{routing::get, Router};

use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/chat", axum::routing::post(chat::chat_handler))
        .route("/ws/events", get(ws::ws_handler))
}