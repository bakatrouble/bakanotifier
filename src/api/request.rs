use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::template::{RenderedMessage, RichTemplate, TemplateError};

use super::error::ApiError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NotifyRequest {
    #[serde(default)]
    pub(super) chat_id: Option<i64>,
    pub(super) template: String,
    #[serde(default)]
    pub(super) values: Value,
}

#[derive(Debug)]
pub(super) struct PreparedMessage {
    pub(super) chat_id: i64,
    pub(super) rendered: RenderedMessage,
}

#[derive(Serialize)]
pub(super) struct SuccessBody {
    pub(super) ok: bool,
    pub(super) message_id: i64,
    pub(super) chat_id: i64,
}

pub(super) fn prepared_from_template(
    chat_id: i64,
    template: RichTemplate,
) -> Result<PreparedMessage, ApiError> {
    let rendered = template
        .into_rendered_message()
        .map_err(template_api_error)?;
    Ok(PreparedMessage { chat_id, rendered })
}

pub(super) fn normalize_values(values: Value) -> Result<Value, ApiError> {
    match values {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(values),
        _ => Err(ApiError::bad_request("values must be an object")),
    }
}

pub(super) fn template_api_error(error: TemplateError) -> ApiError {
    match error {
        TemplateError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "template not found"),
        TemplateError::Render(message)
        | TemplateError::Yaml(message)
        | TemplateError::Invalid(message)
        | TemplateError::Values(message) => ApiError::bad_request(message),
        TemplateError::Io(message) => ApiError::internal(message),
    }
}
