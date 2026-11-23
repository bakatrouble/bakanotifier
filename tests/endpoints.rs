use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header::AUTHORIZATION, header::CONTENT_TYPE};
use axum::routing::post;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;

use bakanotifier::api::build_app;
use bakanotifier::config::{Config, EndpointConfig, OtlpConfig};

const BOT_TOKEN: &str = "123456:TEST-TOKEN";
const API_SECRET: &str = "test-secret";

#[derive(Clone, Default)]
struct Mock {
    rich: Arc<Mutex<Vec<Value>>>,
}

async fn capture_rich(State(mock): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    let chat_id = body["chat_id"].as_i64().unwrap_or_default();
    mock.rich.lock().map(|mut v| v.push(body)).ok();
    Json(json!({
        "ok": true,
        "result": {"message_id": 1, "date": 0, "chat": {"id": chat_id, "type": "private"}}
    }))
}

async fn start_mock() -> (String, Mock) {
    let mock = Mock::default();
    let app = Router::new()
        .route(
            &format!("/bot{BOT_TOKEN}/sendRichMessage"),
            post(capture_rich),
        )
        .with_state(mock.clone());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener should bind");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), mock)
}

fn config(telegram_api_url: String) -> Config {
    Config {
        listen: "127.0.0.1:0".to_string(),
        bot_token: BOT_TOKEN.to_string(),
        default_chat_id: Some(1),
        templates_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates"),
        telegram_api_url,
        api_secrets: vec![API_SECRET.to_string()],
        endpoints: vec![EndpointConfig {
            path: "/docker-updates".to_string(),
            template: "docker-updates".to_string(),
            chat_id: Some(42),
        }],
        otlp: OtlpConfig::default(),
    }
}

fn post_body(path: &str, content_type: &str, body: &str, auth: bool) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(CONTENT_TYPE, content_type);
    if auth {
        builder = builder.header(AUTHORIZATION, format!("Bearer {API_SECRET}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("request should build")
}

async fn status_of(app: &Router, request: Request<Body>) -> StatusCode {
    app.clone()
        .oneshot(request)
        .await
        .expect("router should respond")
        .status()
}

#[tokio::test]
async fn endpoint_uses_preset_template_and_chat_and_whole_body_as_values() {
    let (api_url, mock) = start_mock().await;
    let app = build_app(config(api_url));
    let body = "updates:\n  - stack: forgejo\n    image: codeberg.org/forgejo/forgejo:11.0.3\n    old_version: 11.0.2\n    new_version: 11.0.3\n";

    let status = status_of(
        &app,
        post_body("/docker-updates", "application/yaml", body, true),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let sent = mock.rich.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["chat_id"], 42);
    let html = sent[0]["rich_message"]["html"].as_str().unwrap_or_default();
    assert!(html.contains("Image updates available (1)"), "{html}");
    assert!(html.contains("<summary>forgejo</summary>"), "{html}");
}

#[tokio::test]
async fn endpoint_accepts_json_body_and_requires_auth() {
    let (api_url, _mock) = start_mock().await;
    let app = build_app(config(api_url));
    let body = r#"{"updates": []}"#;

    assert_eq!(
        status_of(
            &app,
            post_body("/docker-updates", "application/json", body, false)
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status_of(
            &app,
            post_body("/docker-updates", "application/json", body, true)
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn endpoint_rejects_non_object_body_and_unknown_path() {
    let (api_url, _mock) = start_mock().await;
    let app = build_app(config(api_url));

    assert_eq!(
        status_of(
            &app,
            post_body("/docker-updates", "application/json", "[1,2]", true)
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status_of(
            &app,
            post_body("/no-such-endpoint", "application/json", "{}", true)
        )
        .await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn notify_route_still_takes_template_in_body() {
    let (api_url, mock) = start_mock().await;
    let app = build_app(config(api_url));
    let body = json!({"template": "docker-updates", "chat_id": 7, "values": {"updates": []}});

    let status = status_of(
        &app,
        post_body("/notify", "application/json", &body.to_string(), true),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mock.rich.lock().expect("lock")[0]["chat_id"], 7);
}
