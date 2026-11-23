use frankenstein::client_reqwest::Bot;
use frankenstein::methods::{SendMessageParams, SendRichMessageParams};
use frankenstein::rich_message::InputRichMessage;
use frankenstein::types::Message;
use frankenstein::{AsyncTelegramApi, ParseMode};
use tracing::Instrument;

use crate::config::Config;
use crate::template::RenderedMessage;

pub struct TelegramClient {
    bot: Bot,
    token: String,
}

impl TelegramClient {
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let base = config.telegram_api_url.trim_end_matches('/');
        let api_url = format!("{base}/bot{}", config.bot_token);
        Self {
            bot: Bot::new_url(api_url),
            token: config.bot_token.clone(),
        }
    }

    pub async fn check_bot(&self) -> Result<(), SendError> {
        self.bot
            .get_me()
            .await
            .map(|_| ())
            .map_err(|error| SendError {
                message: redact(&describe_error(&error), &self.token),
            })
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<ParseMode>,
    ) -> Result<Message, SendError> {
        let params = SendMessageParams::builder()
            .chat_id(chat_id)
            .text(text)
            .maybe_parse_mode(parse_mode)
            .build();

        let span = tracing::info_span!(
            "telegram.send_message",
            chat_id,
            parse_mode = parse_mode.map_or("none", ParseMode::to_str),
            error = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );

        let result = self
            .bot
            .send_message(&params)
            .instrument(span.clone())
            .await;

        match result {
            Ok(response) => Ok(response.result),
            Err(error) => {
                span.record("error", true);
                span.record("otel.status_code", "error");
                Err(SendError {
                    message: redact(&describe_error(&error), &self.token),
                })
            }
        }
    }
    pub async fn send_rendered(
        &self,
        chat_id: i64,
        rendered: RenderedMessage,
    ) -> Result<Message, SendError> {
        match rendered {
            RenderedMessage::Message { text, parse_mode } => {
                self.send_message(chat_id, &text, parse_mode).await
            }
            RenderedMessage::Rich(rich_message) => {
                self.send_rich_message(chat_id, rich_message).await
            }
        }
    }

    pub async fn send_rich_message(
        &self,
        chat_id: i64,
        rich_message: InputRichMessage,
    ) -> Result<Message, SendError> {
        let params = SendRichMessageParams::builder()
            .chat_id(chat_id)
            .rich_message(rich_message)
            .build();

        let span = tracing::info_span!(
            "telegram.send_rich_message",
            chat_id,
            error = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );

        let result = self
            .bot
            .send_rich_message(&params)
            .instrument(span.clone())
            .await;

        match result {
            Ok(response) => Ok(response.result),
            Err(error) => {
                span.record("error", true);
                span.record("otel.status_code", "error");
                Err(SendError {
                    message: redact(&describe_error(&error), &self.token),
                })
            }
        }
    }
}

#[derive(Debug)]
pub struct SendError {
    pub message: String,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SendError {}

fn describe_error(error: &frankenstein::Error) -> String {
    match error {
        frankenstein::Error::Api(response) => response.description.clone(),
        other => other.to_string(),
    }
}

fn redact(text: &str, token: &str) -> String {
    if token.is_empty() {
        return text.to_string();
    }
    text.replace(token, "[redacted]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token_from_error_text() {
        let token = "123456:ABC-DEF";
        let text = format!("request to https://api.telegram.org/bot{token}/sendMessage failed");
        let redacted = redact(&text, token);
        assert!(!redacted.contains(token));
        assert!(redacted.contains("[redacted]"));
    }
}
