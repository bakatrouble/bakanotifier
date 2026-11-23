use frankenstein::ParseMode;
use frankenstein::rich_message::{InputRichBlock, InputRichMessage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::error::TemplateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateMode {
    Text,
    Html,
    Markdown,
    RichHtml,
    RichMarkdown,
    RichBlocks,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RichTemplate {
    pub mode: TemplateMode,
    #[serde(default)]
    pub text: Option<String>,
    /// Array of InputRichBlock objects, validated against frankenstein at render time.
    #[serde(default)]
    pub blocks: Option<Vec<Value>>,
    #[serde(default)]
    pub skip_entity_detection: Option<bool>,
    #[serde(default)]
    pub is_rtl: Option<bool>,
}

impl TemplateMode {
    pub fn is_rich(self) -> bool {
        matches!(self, Self::RichHtml | Self::RichMarkdown | Self::RichBlocks)
    }
}

/// The rendered template converted into the Telegram delivery shape.
#[derive(Debug)]
pub enum RenderedMessage {
    Message {
        text: String,
        parse_mode: Option<ParseMode>,
    },
    Rich(InputRichMessage),
}

impl RenderedMessage {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Message { .. } => "sendMessage",
            Self::Rich(_) => "sendRichMessage",
        }
    }

    /// Build the payload shown by the local example's dry-run mode.
    pub fn payload(&self, chat_id: Option<i64>) -> Value {
        let mut payload = json!({"method": self.method()});
        if let Some(chat_id) = chat_id {
            payload["chat_id"] = json!(chat_id);
        }
        match self {
            Self::Message { text, parse_mode } => {
                payload["text"] = json!(text);
                if let Some(parse_mode) = parse_mode {
                    payload["parse_mode"] = json!(parse_mode.to_str());
                }
            }
            Self::Rich(rich_message) => {
                payload["rich_message"] =
                    serde_json::to_value(rich_message).expect("rich message should serialize");
            }
        }
        payload
    }
}

impl RichTemplate {
    /// Convert the rendered template into a Telegram delivery payload.
    pub fn into_rendered_message(self) -> Result<RenderedMessage, TemplateError> {
        match self.mode {
            TemplateMode::Text => Ok(RenderedMessage::Message {
                text: self.text.ok_or_else(|| {
                    TemplateError::Invalid("text templates require text".to_string())
                })?,
                parse_mode: None,
            }),
            TemplateMode::Html => Ok(RenderedMessage::Message {
                text: self.text.ok_or_else(|| {
                    TemplateError::Invalid("html templates require text".to_string())
                })?,
                parse_mode: Some(ParseMode::Html),
            }),
            TemplateMode::Markdown => Ok(RenderedMessage::Message {
                text: self.text.ok_or_else(|| {
                    TemplateError::Invalid("markdown templates require text".to_string())
                })?,
                parse_mode: Some(ParseMode::MarkdownV2),
            }),
            TemplateMode::RichHtml | TemplateMode::RichMarkdown | TemplateMode::RichBlocks => {
                Ok(RenderedMessage::Rich(self.into_rich_message()?))
            }
        }
    }

    /// Validate mode-specific fields and the shape of rich_blocks blocks.
    pub fn validate(&self) -> Result<(), TemplateError> {
        match self.mode {
            TemplateMode::RichBlocks => {
                if self.text.is_some() {
                    return Err(TemplateError::Invalid(
                        "rich_blocks templates must not set text".to_string(),
                    ));
                }
                if self.skip_entity_detection.is_some() {
                    return Err(TemplateError::Invalid(
                        "rich_blocks templates must not set skip_entity_detection".to_string(),
                    ));
                }
                let blocks = self.blocks.as_ref().ok_or_else(|| {
                    TemplateError::Invalid("rich_blocks templates require blocks".to_string())
                })?;
                serde_json::from_value::<Vec<InputRichBlock>>(Value::Array(blocks.clone()))
                    .map_err(|error| {
                        TemplateError::Invalid(format!("invalid rich_blocks blocks: {error}"))
                    })?;
            }
            TemplateMode::RichHtml | TemplateMode::RichMarkdown => {
                if self.blocks.is_some() {
                    return Err(TemplateError::Invalid(
                        "rich_html and rich_markdown templates must not set blocks".to_string(),
                    ));
                }
                if self.text.is_none() {
                    return Err(TemplateError::Invalid(
                        "rich_html and rich_markdown templates require text".to_string(),
                    ));
                }
            }
            TemplateMode::Text | TemplateMode::Html | TemplateMode::Markdown => {
                if self.blocks.is_some() {
                    return Err(TemplateError::Invalid(
                        "blocks are only valid for rich_blocks mode".to_string(),
                    ));
                }
                if self.skip_entity_detection.is_some() {
                    return Err(TemplateError::Invalid(
                        "skip_entity_detection is only valid for rich modes".to_string(),
                    ));
                }
                if self.is_rtl.is_some() {
                    return Err(TemplateError::Invalid(
                        "is_rtl is only valid for rich modes".to_string(),
                    ));
                }
                if self.text.is_none() {
                    return Err(TemplateError::Invalid(format!(
                        "{} templates require text",
                        mode_name(self.mode)
                    )));
                }
            }
        }
        Ok(())
    }

    /// Convert the rendered template into the payload for sendRichMessage.
    pub fn into_rich_message(self) -> Result<InputRichMessage, TemplateError> {
        let (blocks, html, markdown) = match self.mode {
            TemplateMode::RichBlocks => {
                let blocks = self.blocks.ok_or_else(|| {
                    TemplateError::Invalid("rich_blocks templates require blocks".to_string())
                })?;
                let blocks = serde_json::from_value::<Vec<InputRichBlock>>(Value::Array(blocks))
                    .map_err(|error| {
                        TemplateError::Invalid(format!("invalid rich_blocks blocks: {error}"))
                    })?;
                (Some(blocks), None, None)
            }
            TemplateMode::RichHtml => (None, Some(self.text.unwrap_or_default()), None),
            TemplateMode::RichMarkdown => (None, None, Some(self.text.unwrap_or_default())),
            _ => unreachable!("into_rich_message is only valid for rich modes"),
        };
        Ok(InputRichMessage {
            blocks,
            html,
            markdown,
            media: None,
            is_rtl: self.is_rtl,
            skip_entity_detection: self.skip_entity_detection,
        })
    }
}

fn mode_name(mode: TemplateMode) -> &'static str {
    match mode {
        TemplateMode::Text => "text",
        TemplateMode::Html => "html",
        TemplateMode::Markdown => "markdown",
        TemplateMode::RichHtml => "rich_html",
        TemplateMode::RichMarkdown => "rich_markdown",
        TemplateMode::RichBlocks => "rich_blocks",
    }
}
