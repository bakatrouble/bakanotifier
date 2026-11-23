use crate::config::Config;
use crate::health::SharedHealth;
use crate::telegram::TelegramClient;
use crate::template;

use super::error::ApiError;
use super::request::{
    NotifyRequest, PreparedMessage, normalize_values, prepared_from_template, template_api_error,
};

pub struct AppState {
    pub config: Config,
    pub telegram: TelegramClient,
    pub health: SharedHealth,
}

impl AppState {
    pub fn new(config: Config, health: SharedHealth) -> Self {
        let telegram = TelegramClient::from_config(&config);
        Self {
            config,
            telegram,
            health,
        }
    }

    pub(super) fn prepare(&self, request: NotifyRequest) -> Result<PreparedMessage, ApiError> {
        let values = normalize_values(request.values)?;
        let chat_id = request
            .chat_id
            .or(self.config.default_chat_id)
            .ok_or_else(|| {
                ApiError::bad_request("chat_id is required when no default_chat_id is configured")
            })?;

        let name = request.template;
        if !template::is_valid_template_name(&name) {
            return Err(ApiError::bad_request(
                "template name must match ^[A-Za-z0-9_-]+$",
            ));
        }

        let span = tracing::info_span!(
            "template.render",
            template = %name,
            error = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );
        let rendered =
            span.in_scope(|| template::load_and_render(&self.config.templates_dir, &name, &values));
        let rendered = match rendered {
            Ok(rendered) => rendered,
            Err(error) => {
                span.record("error", true);
                span.record("otel.status_code", "error");
                return Err(template_api_error(error));
            }
        };
        prepared_from_template(chat_id, rendered)
    }
}
