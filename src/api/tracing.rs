use axum::extract::{MatchedPath, Request};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::propagation::Extractor;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|key| key.as_str()).collect()
    }
}

pub(super) async fn trace_http(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| path.clone(), |matched| matched.as_str().to_string());
    let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });

    let span = tracing::info_span!(
        "http.server",
        otel.kind = "server",
        http.request.method = %method,
        http.route = %route,
        url.path = %path,
        http.response.status_code = tracing::field::Empty,
        error = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    if let Err(error) = span.set_parent(parent) {
        tracing::debug!(%error, "failed to set remote trace parent");
    }

    let response = next.run(request).instrument(span.clone()).await;
    let status = response.status();
    span.record("http.response.status_code", i64::from(status.as_u16()));
    if status.is_server_error() {
        span.record("error", true);
        span.record("otel.status_code", "error");
    }
    response
}
