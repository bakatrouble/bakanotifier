use anyhow::{Context, Result, bail};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::Config;

const PROTOCOL_HTTP_PROTOBUF: &str = "http/protobuf";
const DEFAULT_SERVICE_NAME: &str = "bakanotifier";

pub struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

#[derive(Debug, Clone)]
pub struct OtlpSettings {
    pub endpoint: String,
    pub service_name: String,
    pub sampling_ratio: f64,
}

impl Telemetry {
    /// Install the tracing subscriber and, when configured, the OTLP HTTP exporter.
    ///
    /// Precedence:
    /// - endpoint: OTEL_EXPORTER_OTLP_ENDPOINT, then otlp.endpoint in the config
    /// - service name: OTEL_SERVICE_NAME, then otlp.service_name, then "bakanotifier"
    /// - protocol: OTEL_EXPORTER_OTLP_PROTOCOL must be unset or "http/protobuf"
    pub fn init(config: &Config) -> Result<Self> {
        let settings = resolve_settings(config)?;
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let fmt_layer = tracing_subscriber::fmt::layer();

        let Some(settings) = settings else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
            install_propagation();
            return Ok(Self { provider: None });
        };

        let traces_endpoint = traces_endpoint(&settings.endpoint);
        let exporter = SpanExporter::builder()
            .with_http()
            .with_endpoint(traces_endpoint)
            .build()
            .context("building OTLP HTTP span exporter")?;

        let batch_processor = BatchSpanProcessor::builder(exporter, runtime::Tokio).build();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(batch_processor)
            .with_resource(
                Resource::builder()
                    .with_service_name(settings.service_name.clone())
                    .build(),
            )
            .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
                settings.sampling_ratio,
            ))))
            .build();
        let tracer = provider.tracer(DEFAULT_SERVICE_NAME);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();

        opentelemetry::global::set_tracer_provider(provider.clone());
        install_propagation();

        tracing::info!(
            endpoint = %settings.endpoint,
            service_name = %settings.service_name,
            sampling_ratio = settings.sampling_ratio,
            "OTLP trace export enabled"
        );

        Ok(Self {
            provider: Some(provider),
        })
    }

    /// Flush buffered spans and shut down the provider.
    pub fn shutdown(&self) {
        if let Some(provider) = &self.provider
            && let Err(error) = provider.shutdown()
        {
            tracing::warn!(%error, "failed to shut down OTLP tracer provider");
        }
    }
}

pub fn install_propagation() {
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
}

fn resolve_settings(config: &Config) -> Result<Option<OtlpSettings>> {
    if let Some(protocol) = env_non_empty("OTEL_EXPORTER_OTLP_PROTOCOL")
        && !protocol.eq_ignore_ascii_case(PROTOCOL_HTTP_PROTOBUF)
    {
        bail!(
            "unsupported OTEL_EXPORTER_OTLP_PROTOCOL {protocol:?}: only {PROTOCOL_HTTP_PROTOBUF} is supported"
        );
    }

    let endpoint = env_non_empty("OTEL_EXPORTER_OTLP_ENDPOINT").or_else(|| {
        config
            .otlp
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .map(ToString::to_string)
    });
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };

    let service_name = env_non_empty("OTEL_SERVICE_NAME")
        .or_else(|| Some(config.otlp.service_name.clone()))
        .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string());
    let sampling_ratio = config.otlp.sampling_ratio;

    Ok(Some(OtlpSettings {
        endpoint,
        service_name,
        sampling_ratio,
    }))
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The exporter treats a programmatically supplied endpoint as the full URL,
/// so append the OTLP/HTTP traces path to the configured base URL.
fn traces_endpoint(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/v1/traces") {
        base.to_string()
    } else {
        format!("{base}/v1/traces")
    }
}

#[cfg(test)]
mod tests {
    use super::traces_endpoint;

    #[test]
    fn appends_traces_path_to_base_endpoint() {
        assert_eq!(
            traces_endpoint("http://collector:4318"),
            "http://collector:4318/v1/traces"
        );
        assert_eq!(
            traces_endpoint("http://collector:4318/"),
            "http://collector:4318/v1/traces"
        );
        assert_eq!(
            traces_endpoint("http://collector:4318/v1/traces"),
            "http://collector:4318/v1/traces"
        );
    }
}
