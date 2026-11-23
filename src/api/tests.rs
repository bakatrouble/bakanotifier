use std::path::PathBuf;

use axum::http::StatusCode;
use frankenstein::ParseMode;
use serde_json::{Value, json};

use crate::config::Config;
use crate::health::shared_health;

use crate::template::RenderedMessage;

use super::request::NotifyRequest;
use super::state::AppState;

fn state(default_chat_id: Option<i64>) -> AppState {
    let config = Config {
        listen: "127.0.0.1:0".to_string(),
        bot_token: "123456:ABC-DEF".to_string(),
        default_chat_id,
        templates_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates"),
        telegram_api_url: "http://127.0.0.1:1".to_string(),
        api_secrets: vec!["change-me".to_string()],
        otlp: crate::config::OtlpConfig::default(),
    };
    AppState::new(config, shared_health())
}

fn request(value: Value) -> Result<NotifyRequest, serde_json::Error> {
    serde_json::from_value(value)
}

fn prepare(template: &str, values: Value) -> Result<RenderedMessage, super::error::ApiError> {
    let request = request(json!({
        "chat_id": 1,
        "template": template,
        "values": values
    }))
    .unwrap();
    state(None)
        .prepare(request)
        .map(|prepared| prepared.rendered)
}

#[test]
fn template_is_required() {
    assert!(request(json!({"chat_id": 1, "values": {}})).is_err());
}

#[test]
fn text_template_uses_no_parse_mode() {
    let content = prepare("text", json!({"message": "hello"})).unwrap();
    match content {
        RenderedMessage::Message { parse_mode, text } => {
            assert_eq!(parse_mode, None);
            assert_eq!(text, "hello");
        }
        RenderedMessage::Rich(_) => panic!("text template should produce sendMessage"),
    }
}

#[test]
fn html_template_uses_html_parse_mode() {
    let content = prepare("html", json!({"message": "<b>hello</b>"})).unwrap();
    match content {
        RenderedMessage::Message { parse_mode, text } => {
            assert_eq!(parse_mode, Some(ParseMode::Html));
            assert_eq!(text, "<b>hello</b>");
        }
        RenderedMessage::Rich(_) => panic!("html template should produce sendMessage"),
    }
}

#[test]
fn markdown_template_uses_markdown_v2() {
    let content = prepare("markdown", json!({"message": "**hello**"})).unwrap();
    match content {
        RenderedMessage::Message { parse_mode, text } => {
            assert_eq!(parse_mode, Some(ParseMode::MarkdownV2));
            assert_eq!(text, "**hello**");
        }
        RenderedMessage::Rich(_) => panic!("markdown template should produce sendMessage"),
    }
}

#[test]
fn default_template_requires_message_value() {
    let error = prepare("text", json!({})).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn chat_id_falls_back_to_default() {
    let request = request(json!({"template": "text", "values": {"message": "hello"}})).unwrap();
    let prepared = state(Some(42)).prepare(request).unwrap();
    assert_eq!(prepared.chat_id, 42);
}

#[test]
fn chat_id_is_required_without_default() {
    let request = request(json!({"template": "text", "values": {"message": "hello"}})).unwrap();
    let error = state(None).prepare(request).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error.message.contains("chat_id"));
}

#[test]
fn parse_mode_and_message_fields_are_rejected() {
    assert!(request(json!({"chat_id": 1, "parse_mode": "text", "template": "text"})).is_err());
    assert!(request(json!({"chat_id": 1, "template": "text", "message": "hello"})).is_err());
}

#[test]
fn extra_field_is_rejected() {
    let result = request(json!({"chat_id": 1, "template": "text", "extra": true}));
    assert!(result.is_err());
}

#[test]
fn values_must_be_an_object() {
    let request = request(json!({
        "chat_id": 1,
        "template": "example",
        "values": [1, 2]
    }))
    .unwrap();
    let error = state(None).prepare(request).unwrap_err();
    assert!(error.message.contains("values"));
}

#[test]
fn rich_message_uses_template() {
    let request = request(json!({
        "chat_id": 1,
        "template": "example",
        "values": {"title": "Deploy failed", "items": ["api", "worker"], "urgent": true}
    }))
    .unwrap();
    let prepared = state(None).prepare(request).unwrap();
    assert_eq!(prepared.chat_id, 1);
    match prepared.rendered {
        RenderedMessage::Rich(rich) => {
            assert_eq!(
                rich.html.as_deref(),
                Some("<b>Deploy failed</b>\n- api\n- worker")
            );
            assert!(rich.markdown.is_none());
            assert_eq!(rich.skip_entity_detection, Some(true));
        }
        RenderedMessage::Message { .. } => panic!("rich template should produce a rich message"),
    }
}

#[test]
fn path_traversal_template_name_is_rejected() {
    let request = request(json!({
        "chat_id": 1,
        "template": "../example"
    }))
    .unwrap();
    let error = state(None).prepare(request).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}
