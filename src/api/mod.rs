mod auth;
mod error;
mod health;
mod notify;
mod request;
mod state;
#[cfg(test)]
mod tests;
mod tracing;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::config::Config;
use crate::health::{SharedHealth, shared_health};

pub use self::state::AppState;

use self::health::health_handler;
use self::notify::{endpoint_notify, notify};
use self::tracing::trace_http;

pub fn build_app(config: Config) -> Router {
    build_app_with_health(config, shared_health())
}

pub fn build_app_with_health(config: Config, health: SharedHealth) -> Router {
    let endpoints = config.endpoints.clone();
    let state = Arc::new(AppState::new(config, health));
    let mut router = Router::new()
        .route("/health", get(health_handler))
        .route("/notify", post(notify));
    for endpoint in endpoints {
        let path = endpoint.path.clone();
        router = router.route(
            &path,
            post(
                move |State(state): State<Arc<AppState>>,
                      headers: HeaderMap,
                      body: Result<Bytes, BytesRejection>| {
                    let endpoint = endpoint.clone();
                    async move { endpoint_notify(state, headers, body, endpoint).await }
                },
            ),
        );
    }
    router
        .fallback(not_found)
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn(trace_http))
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
}
