use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};

use crate::health::health_response;

use super::state::AppState;

pub(super) async fn health_handler(State(state): State<Arc<AppState>>) -> Response {
    let health = state.health.lock().expect("health state lock");
    let (status, body) = health_response(&health);
    (status, Json(body)).into_response()
}
