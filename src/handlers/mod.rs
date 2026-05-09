use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub mod audit;
pub mod auth;
pub mod docker;
pub mod logs;
pub mod metrics;
pub mod pm2;
pub mod processes;
pub mod services;
pub mod static_files;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/metrics", get(metrics::current))
        .route("/metrics/stream", get(metrics::stream))
        .route("/processes", get(processes::list))
        .route("/pm2", get(pm2::list))
        .route("/docker", get(docker::list))
        .route("/services", get(services::list))
        .route("/audit", get(audit::list))
        .route("/logs/pm2/:name", get(logs::ws_pm2))
        .route("/logs/docker/:id", get(logs::ws_docker))
        .route("/logs/journal/:unit", get(logs::ws_journal));

    Router::new()
        .nest("/api", api)
        .fallback(static_files::handler)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
