mod config;
mod db;
mod error;
mod models;
mod routes;
mod state;
mod websocket;

use axum::{routing::{get, post}, Router};
use config::Config;
use routes::{active_chats_handler, block_handler, create_group_handler, delete_account_handler, history_handler, message_search_handler, login_handler, register_handler, search_handler, unblock_handler, ws_route_handler};
use state::AppState;
use std::{net::SocketAddr, sync::Arc};
use tower_http::{cors::{Any, CorsLayer}, services::{ServeDir, ServeFile}, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "messenger_server=info,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();
    let state = Arc::new(AppState::new(&config.database_path));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let static_files = ServeDir::new("static")
        .not_found_service(ServeFile::new("static/index.html"));

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/register", post(register_handler))
        .route("/api/login", post(login_handler))
        .route("/api/search", get(search_handler))
        .route("/api/messages", get(history_handler))
        .route("/api/messages/search", get(message_search_handler))
        .route("/api/chats", get(active_chats_handler))
        .route("/api/groups", post(create_group_handler))
        .route("/api/block", post(block_handler))
        .route("/api/unblock", post(unblock_handler))
        .route("/api/account/delete", post(delete_account_handler))
        .route("/ws", get(ws_route_handler))
        .fallback_service(static_files)
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("messenger started: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind failed");
    axum::serve(listener, app).await.expect("server failed");
}
