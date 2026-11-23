use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use opentelemetry::trace::{SpanId, Status, TraceId, TracerProvider as _};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use bakanotifier::Config;
use bakanotifier::api::build_app;
use bakanotifier::telemetry::install_propagation;

const BOT_TOKEN: &str = "123456:TEST-TOKEN";
const API_SECRET: &str = "test-secret";
const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT_SPAN_ID: &str = "00f067aa0ba902b7";
const MESSAGE_MARKER: &str = "super-secret-message-content";

async fn capture_message(
    State(received): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let chat_id = body.get("chat_id").and_then(Value::as_i64).unwrap_or(0);
    received.lock().expect("mock state lock").push(body);
    Json(json!({
        "ok": true,
        "result": {
            "message_id": 1,
            "date": 0,
            "chat": {"id": chat_id, "type": "private"}
        }
    }))
}

async fn start_mock_telegram() -> (String, Arc<Mutex<Vec<Value>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            &format!("/bot{BOT_TOKEN}/sendMessage"),
            post(capture_message),
        )
        .route(
            &format!("/bot{BOT_TOKEN}/sendRichMessage"),
            post(capture_message),
        )
        .with_state(received.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener should bind");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock server should run");
    });

    (format!("http://{address}"), received)
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

async fn post_notify(app: &Router, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/notify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {API_SECRET}"))
        .header("traceparent", format!("00-{TRACE_ID}-{PARENT_SPAN_ID}-01"))
        .body(axum::body::Body::from(body.to_string()))
        .expect("request should build");

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should run");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let json = serde_json::from_slice(&bytes).expect("response should be JSON");
    (status, json)
}

fn attr<'a>(span: &'a SpanData, key: &str) -> Option<&'a opentelemetry::Value> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| &kv.value)
}

fn attr_str<'a>(span: &'a SpanData, key: &str) -> Option<&'a str> {
    match attr(span, key) {
        Some(opentelemetry::Value::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn attr_i64(span: &SpanData, key: &str) -> Option<i64> {
    match attr(span, key) {
        Some(opentelemetry::Value::I64(value)) => Some(*value),
        _ => None,
    }
}

fn assert_span_tree(spans: &[SpanData]) {
    let by_name = |name: &str| {
        spans
            .iter()
            .find(|span| span.name.as_ref() == name)
            .unwrap_or_else(|| panic!("missing span {name}; got {:?}", names(spans)))
    };

    let http = by_name("http.server");
    assert_eq!(http.span_kind, opentelemetry::trace::SpanKind::Server);
    assert_eq!(
        http.span_context.trace_id(),
        TraceId::from_hex(TRACE_ID).unwrap()
    );
    assert_eq!(
        http.parent_span_id,
        SpanId::from_hex(PARENT_SPAN_ID).unwrap()
    );
    assert!(http.parent_span_is_remote);
    assert_eq!(attr_str(http, "http.request.method"), Some("POST"));
    assert_eq!(attr_str(http, "http.route"), Some("/notify"));
    assert_eq!(attr_i64(http, "http.response.status_code"), Some(200));

    let notify = by_name("notify");
    assert_eq!(notify.parent_span_id, http.span_context.span_id());
    assert_eq!(attr_i64(notify, "chat_id"), Some(77));
    assert_eq!(attr_str(notify, "template"), Some("example"));

    let render = by_name("template.render");
    assert_eq!(render.parent_span_id, notify.span_context.span_id());
    assert_eq!(attr_str(render, "template"), Some("example"));
    assert!(!matches!(render.status, Status::Error { .. }));

    let telegram = by_name("telegram.send_rich_message");
    assert_eq!(telegram.parent_span_id, notify.span_context.span_id());
    assert_eq!(attr_i64(telegram, "chat_id"), Some(77));
    assert!(!matches!(telegram.status, Status::Error { .. }));
}

fn assert_no_sensitive_attributes(spans: &[SpanData]) {
    for span in spans {
        for attribute in &span.attributes {
            let rendered = format!("{:?}", attribute.value);
            assert!(
                !rendered.contains(MESSAGE_MARKER),
                "span {} leaked message text in attribute {}",
                span.name,
                attribute.key
            );
            assert!(
                !rendered.contains("sda 97%"),
                "span {} leaked template values in attribute {}",
                span.name,
                attribute.key
            );
            assert!(
                !rendered.contains(BOT_TOKEN),
                "span {} leaked the bot token in attribute {}",
                span.name,
                attribute.key
            );
            assert!(
                !rendered.contains(API_SECRET),
                "span {} leaked an api secret in attribute {}",
                span.name,
                attribute.key
            );
        }
    }
}

fn assert_failed_spans(spans: &[SpanData]) {
    let failed_notify = spans
        .iter()
        .find(|span| {
            span.name.as_ref() == "notify" && attr_str(span, "template") == Some("missing-template")
        })
        .expect("failed notify span should be exported");
    assert!(
        matches!(failed_notify.status, Status::Error { .. }),
        "failed notify span should be marked as an error"
    );

    let failed_render = spans
        .iter()
        .find(|span| {
            span.name.as_ref() == "template.render"
                && attr_str(span, "template") == Some("missing-template")
        })
        .expect("failed template span should be exported");
    assert!(matches!(failed_render.status, Status::Error { .. }));
}

#[tokio::test]
async fn notify_emits_expected_spans_without_message_text() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let tracer = provider.tracer("bakanotifier-test");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    tracing::subscriber::set_global_default(subscriber).expect("subscriber should install");
    install_propagation();

    let (api_url, received) = start_mock_telegram().await;
    let app = build_app(test_config(api_url));

    let (status, body) = post_notify(
        &app,
        json!({
            "chat_id": 77,
            "template": "example",
            "values": {
                "title": MESSAGE_MARKER,
                "items": ["sda 97%", "sdb 12%"],
                "urgent": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(received.lock().expect("mock state lock").len(), 1);

    let spans = exporter
        .get_finished_spans()
        .expect("spans should be exported");
    assert_span_tree(&spans);
    assert_no_sensitive_attributes(&spans);

    let (status, body) =
        post_notify(&app, json!({"chat_id": 77, "template": "missing-template"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");

    let spans = exporter
        .get_finished_spans()
        .expect("spans should be exported");
    assert_failed_spans(&spans);
}

fn names(spans: &[SpanData]) -> Vec<String> {
    spans.iter().map(|span| span.name.to_string()).collect()
}
