use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::rejection::BytesRejection;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use tracing::{Instrument, info, warn};

use crate::config::EndpointConfig;
use crate::parsing::{parse_json_value, parse_yaml_value};
use crate::telegram::SendError;

use super::auth::is_authorized;
use super::error::error_response;
use super::request::{NotifyRequest, SuccessBody};
use super::state::AppState;

#[derive(Debug, Clone, Copy)]
enum BodyFormat {
    Json,
    Yaml,
}

impl BodyFormat {
    fn name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
        }
    }
}

fn body_format(headers: &HeaderMap) -> Result<BodyFormat, (StatusCode, String)> {
    let Some(content_type) = headers.get(CONTENT_TYPE) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "missing Content-Type; use application/json or application/yaml".to_string(),
        ));
    };
    let content_type = content_type.to_str().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "invalid Content-Type header".to_string(),
        )
    })?;
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();

    if media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
    {
        Ok(BodyFormat::Json)
    } else if matches!(
        media_type.as_str(),
        "application/yaml" | "application/x-yaml" | "text/yaml"
    ) || (media_type.starts_with("application/") && media_type.ends_with("+yaml"))
    {
        Ok(BodyFormat::Yaml)
    } else {
        Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("unsupported Content-Type {content_type:?}"),
        ))
    }
}

pub(super) async fn notify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    handle(state, headers, body, Source::Request).await
}

/// Handles a configured endpoint: preset template and chat id, whole body as values.
pub(super) async fn endpoint_notify(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
    endpoint: EndpointConfig,
) -> Response {
    handle(state, headers, body, Source::Endpoint(endpoint)).await
}

enum Source {
    Request,
    Endpoint(EndpointConfig),
}

async fn handle(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
    route: Source,
) -> Response {
    if !is_authorized(&headers, &state.config.api_secrets) {
        return error_response(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    }

    let format = match body_format(&headers) {
        Ok(format) => format,
        Err((status, message)) => return error_response(status, message),
    };
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(rejection) => {
            return error_response(StatusCode::BAD_REQUEST, rejection.body_text());
        }
    };
    let Ok(source) = std::str::from_utf8(&bytes) else {
        return error_response(StatusCode::BAD_REQUEST, "request body is not valid UTF-8");
    };
    let parsed = match format {
        BodyFormat::Json => parse_json_value(source),
        BodyFormat::Yaml => parse_yaml_value(source),
    };
    let value = match parsed {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid {} request body: {error}", format.name()),
            );
        }
    };
    let request: NotifyRequest = match route {
        Source::Request => match serde_json::from_value(value) {
            Ok(request) => request,
            Err(error) => {
                return error_response(StatusCode::BAD_REQUEST, error.to_string());
            }
        },
        Source::Endpoint(endpoint) => NotifyRequest {
            chat_id: endpoint.chat_id,
            template: endpoint.template,
            values: value,
        },
    };

    let log_chat_id = request.chat_id.or(state.config.default_chat_id);
    let log_template = request.template.clone();

    let span = tracing::info_span!(
        "notify",
        chat_id = tracing::field::Empty,
        template = log_template,
        error = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    if let Some(chat_id) = log_chat_id {
        span.record("chat_id", chat_id);
    }

    let response = handle_notification(state, request, log_chat_id, log_template)
        .instrument(span.clone())
        .await;

    if !response.status().is_success() {
        span.record("error", true);
        span.record("otel.status_code", "error");
    }
    response
}

async fn handle_notification(
    state: Arc<AppState>,
    request: NotifyRequest,
    log_chat_id: Option<i64>,
    log_template: String,
) -> Response {
    let prepared = match state.prepare(request) {
        Ok(prepared) => prepared,
        Err(error) => {
            warn!(
                chat_id = ?log_chat_id,
                template = log_template,
                status = error.status.as_u16(),
                "notification request rejected"
            );
            return error_response(error.status, error.message);
        }
    };

    match state
        .telegram
        .send_rendered(prepared.chat_id, prepared.rendered)
        .await
    {
        Ok(message) => {
            info!(
                chat_id = prepared.chat_id,
                template = log_template,
                "notification sent"
            );
            (
                StatusCode::OK,
                Json(SuccessBody {
                    ok: true,
                    message_id: i64::from(message.message_id),
                    chat_id: message.chat.id,
                }),
            )
                .into_response()
        }
        Err(SendError { message }) => {
            warn!(
                chat_id = prepared.chat_id,
                template = log_template,
                error = %message,
                "telegram send failed"
            );
            error_response(StatusCode::BAD_GATEWAY, message)
        }
    }
}
