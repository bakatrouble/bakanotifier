use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tower::ServiceExt;

use bakanotifier::Config;
use bakanotifier::api::{build_app, build_app_with_health};
use bakanotifier::health::{poll_bot_health, shared_health};
use bakanotifier::telegram::TelegramClient;
use bakanotifier::template::load_and_render;

const BOT_TOKEN: &str = "123456:TEST-TOKEN";
const API_SECRET: &str = "test-secret";

#[derive(Clone, Default)]
struct MockState {
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    get_me_calls: Arc<AtomicUsize>,
}

impl MockState {
    fn record(&self, path: &str, body: Value) -> i64 {
        let chat_id = body.get("chat_id").and_then(Value::as_i64).unwrap_or(0);
        self.requests
            .lock()
            .expect("mock state lock")
            .push((path.to_string(), body));
        chat_id
    }

    fn last_request(&self) -> (String, Value) {
        self.requests
            .lock()
            .expect("mock state lock")
            .last()
            .cloned()
            .expect("mock should receive a message")
    }
}

async fn capture_send_message(
    State(state): State<MockState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let chat_id = state.record("/sendMessage", body);
    Json(message_response(chat_id))
}

async fn capture_send_rich_message(
    State(state): State<MockState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let chat_id = state.record("/sendRichMessage", body);
    Json(message_response(chat_id))
}

fn message_response(chat_id: i64) -> Value {
    json!({
        "ok": true,
        "result": {
            "message_id": 1,
            "date": 0,
            "chat": {"id": chat_id, "type": "private"}
        }
    })
}

async fn get_me(State(state): State<MockState>) -> Json<Value> {
    state.get_me_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "ok": true,
        "result": {"id": 1, "is_bot": true, "first_name": "bakanotifier"}
    }))
}

async fn start_mock_telegram() -> (String, MockState, JoinHandle<()>) {
    let state = MockState::default();
    let app = Router::new()
        .route(
            &format!("/bot{BOT_TOKEN}/sendMessage"),
            post(capture_send_message),
        )
        .route(
            &format!("/bot{BOT_TOKEN}/sendRichMessage"),
            post(capture_send_rich_message),
        )
        .route(&format!("/bot{BOT_TOKEN}/getMe"), post(get_me))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener should bind");
    let address = listener.local_addr().expect("mock address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock server should run");
    });

    (format!("http://{address}"), state, handle)
}

fn test_config(telegram_api_url: String) -> Config {
    Config {
        listen: "127.0.0.1:0".to_string(),
        bot_token: BOT_TOKEN.to_string(),
        default_chat_id: None,
        templates_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates"),
        telegram_api_url,
        api_secrets: vec![API_SECRET.to_string()],
        otlp: bakanotifier::config::OtlpConfig::default(),
    }
}

async fn post_notify(
    app: &Router,
    authorization: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    post_raw(
        app,
        authorization,
        Some("application/json"),
        &body.to_string(),
    )
    .await
}

async fn post_raw(
    app: &Router,
    authorization: Option<&str>,
    content_type: Option<&str>,
    body: &str,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("POST").uri("/notify");
    if let Some(token) = authorization {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    let request = builder
        .body(axum::body::Body::from(body.to_string()))
        .expect("request should build");

    send_request(app, request).await
}

async fn get_json(app: &Router, path: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .body(axum::body::Body::empty())
        .expect("request should build");
    send_request(app, request).await
}

async fn send_request(app: &Router, request: Request<axum::body::Body>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should run");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("response should be JSON"),
    )
}

#[tokio::test]
async fn end_to_end_against_mock_telegram() {
    let (api_url, mock, mock_task) = start_mock_telegram().await;
    let app = build_app(test_config(api_url));

    let (status, body) = get_json(&app, "/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "starting");
    assert_eq!(body["telegram"]["reachable"], false);
    assert_eq!(mock.get_me_calls.load(Ordering::SeqCst), 0);

    let (status, body) = post_notify(
        &app,
        None,
        json!({"chat_id": 1, "template": "text", "values": {"message": "hello"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some("wrong"),
        json!({"chat_id": 1, "template": "text", "values": {"message": "hello"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 123, "template": "text", "values": {"message": "hello"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["message_id"], 1);
    assert_eq!(body["chat_id"], 123);

    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["chat_id"], 123);
        assert_eq!(last["text"], "hello");
        assert!(last.get("parse_mode").is_none());
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 123, "template": "html", "values": {"message": "<b>hello</b>"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["text"], "<b>hello</b>");
        assert_eq!(last["parse_mode"], "HTML");
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 123, "template": "markdown", "values": {"message": "**hello**"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["text"], "**hello**");
        assert_eq!(last["parse_mode"], "MarkdownV2");
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({
            "chat_id": 123,
            "template": "example",
            "values": {
                "title": "Deploy failed",
                "items": ["api", "worker"],
                "urgent": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        assert_eq!(
            last["rich_message"]["html"],
            "<b>Deploy failed</b>\n- api\n- worker"
        );
        assert!(last["rich_message"].get("markdown").is_none());
        assert_eq!(last["rich_message"]["skip_entity_detection"], true);
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({
            "chat_id": 123,
            "template": "deploy",
            "values": {
                "failed": true,
                "service": "api",
                "version": "1.0",
                "reason": "timeout"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        assert_eq!(
            last["rich_message"]["html"],
            "<b>Deploy failed</b>: api 1.0\n<i>timeout</i>"
        );
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({
            "chat_id": 123,
            "template": "rich-markdown-kitchen-sink",
            "values": {
                "title": "Markdown kitchen sink",
                "items": ["alpha", "beta"],
                "note": "A note",
                "skip_entity_detection": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        let markdown = last["rich_message"]["markdown"]
            .as_str()
            .expect("rich markdown body should be a string");
        assert!(markdown.starts_with("# Markdown kitchen sink"));
        assert!(markdown.contains("- alpha\n- beta"));
        assert!(last["rich_message"].get("html").is_none());
    }

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({
            "chat_id": 123,
            "template": "rich-blocks-example",
            "values": {
                "title": "Rich blocks example",
                "items": ["first item", "second item"],
                "footer": "Generated from JSON-e"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        let blocks = last["rich_message"]["blocks"]
            .as_array()
            .expect("rich blocks should be an array");
        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[0]["type"], "heading");
        assert_eq!(blocks[2]["type"], "list");
        assert_eq!(blocks[3]["type"], "footer");
        assert_eq!(blocks[4]["type"], "divider");
        assert!(last["rich_message"].get("html").is_none());
        assert!(last["rich_message"].get("markdown").is_none());
    }

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("application/yaml"),
        "chat_id: 123\ntemplate: text\nvalues:\n  message: hello\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["text"], "hello");
    }

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("application/yaml"),
        "chat_id: 123\ntemplate: example\nvalues:\n  title: &title Disk alert\n  items: [*title, \"sdb 12%\"]\n  urgent: true\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        assert_eq!(
            last["rich_message"]["html"],
            "<b>Disk alert</b>\n- Disk alert\n- sdb 12%"
        );
    }

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("application/yaml"),
        "chat_id: 123\ntemplate: text\nvalues:\n  message: yes\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (_, last) = mock.last_request();
        assert_eq!(last["text"], "yes");
    }

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("application/yaml"),
        "chat_id: 1\ntemplate: text\nvalues: !custom\n  message: hello\n",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("text/plain"),
        "{\"chat_id\": 1, \"template\": \"text\"}",
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "body: {body}");

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        None,
        "{\"chat_id\": 1, \"template\": \"text\"}",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "does-not-exist"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["ok"], false);

    let (status, body) =
        post_notify(&app, Some(API_SECRET), json!({"chat_id": 1, "values": {}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "text", "values": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|e| e.contains("message"))
    );

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "text", "values": {"message": 42}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "text", "values": {"message": "hello", "extra": true}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "text", "values": {"message": "hello"}, "parse_mode": "text"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "text", "message": "hello"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "../example"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    let (status, body) = post_notify(
        &app,
        Some(API_SECRET),
        json!({"chat_id": 1, "template": "example", "values": [1, 2]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);

    mock_task.abort();
}

#[tokio::test]
async fn health_reports_starting_without_completed_check() {
    // No poller has run, so the handler must not contact Telegram.
    let app = build_app(test_config("http://127.0.0.1:1".to_string()));

    let (status, body) = get_json(&app, "/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "starting");
    assert_eq!(body["telegram"]["reachable"], false);
    assert!(body["telegram"]["last_checked_unix"].is_null());
}

#[tokio::test]
async fn health_uses_background_poller_and_not_per_request_telegram_calls() {
    let (api_url, mock, mock_task) = start_mock_telegram().await;
    let config = test_config(api_url);
    let telegram = TelegramClient::from_config(&config);
    let health = shared_health();
    let (shutdown, shutdown_rx) = watch::channel(false);
    let poller = tokio::spawn(poll_bot_health(telegram, health.clone(), shutdown_rx));
    let app = build_app_with_health(config, health);

    let mut response = (StatusCode::SERVICE_UNAVAILABLE, Value::Null);
    for _ in 0..100 {
        response = get_json(&app, "/health").await;
        if response.0 == StatusCode::OK {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1["status"], "ok");
    assert_eq!(response.1["telegram"]["reachable"], true);

    let calls_after_poll = mock.get_me_calls.load(Ordering::SeqCst);
    assert!(calls_after_poll >= 1, "poller should call getMe");

    for _ in 0..5 {
        let (status, body) = get_json(&app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }
    assert_eq!(
        mock.get_me_calls.load(Ordering::SeqCst),
        calls_after_poll,
        "/health must not call getMe"
    );

    let (status, body) = get_json(&app, "/unknown").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"error": "not found"}));

    let _ = shutdown.send(true);
    poller.await.expect("poller should stop");
    mock_task.abort();
}

#[tokio::test]
async fn send_helper_dispatches_rendered_templates_to_mock_telegram() {
    let (api_url, mock, mock_task) = start_mock_telegram().await;
    let config = test_config(api_url);
    let client = TelegramClient::from_config(&config);
    let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");

    let text = load_and_render(&templates, "text", &json!({"message": "hello"}))
        .expect("text template should render")
        .into_rendered_message()
        .expect("text template should convert");
    client
        .send_rendered(55, text)
        .await
        .expect("text send should succeed");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["chat_id"], 55);
        assert_eq!(last["text"], "hello");
        assert!(last.get("parse_mode").is_none());
    }

    let example_values = serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/values/example.json"),
        )
        .expect("example values should be readable"),
    )
    .expect("example values should be valid JSON");
    let rich = load_and_render(&templates, "example", &example_values)
        .expect("example should render")
        .into_rendered_message()
        .expect("example should convert");
    client
        .send_rendered(55, rich)
        .await
        .expect("rich send should succeed");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendRichMessage");
        assert_eq!(last["chat_id"], 55);
        assert_eq!(
            last["rich_message"]["html"],
            "<b>Disk alert</b>\n- sda 97%\n- sdb 12%"
        );
    }

    mock_task.abort();
}

#[tokio::test]
async fn yaml_numeric_mapping_keys_are_coerced_and_usable() {
    let (api_url, mock, mock_task) = start_mock_telegram().await;
    let templates = std::env::temp_dir().join(format!(
        "bakanotifier-yaml-key-coercion-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&templates).expect("temp templates dir");
    std::fs::write(
        templates.join("numeric-keys.yaml"),
        r#"mode: text
text:
  $eval: "items[0]['1']"
values:
  type: object
  properties:
    items:
      type: array
      items:
        type: object
  required:
    - items
  additionalProperties: false
"#,
    )
    .expect("template should be written");

    let mut config = test_config(api_url);
    config.templates_dir = templates.clone();
    let app = build_app(config);

    let (status, body) = post_raw(
        &app,
        Some(API_SECRET),
        Some("application/yaml"),
        "chat_id: 123\ntemplate: numeric-keys\nvalues:\n  items:\n    - 1: alpha\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    {
        let (path, last) = mock.last_request();
        assert_eq!(path, "/sendMessage");
        assert_eq!(last["text"], "alpha");
    }

    std::fs::remove_dir_all(&templates).ok();
    mock_task.abort();
}
