use std::path::PathBuf;

pub const CONFIG_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/schemas/config.schema.json"
));

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_TELEGRAM_API_URL: &str = "https://api.telegram.org";

fn default_listen() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_bot_token() -> String {
    String::new()
}

fn default_templates_dir() -> PathBuf {
    PathBuf::new()
}

fn default_telegram_api_url() -> String {
    DEFAULT_TELEGRAM_API_URL.to_string()
}

fn default_service_name() -> String {
    "bakanotifier".to_string()
}

fn default_sampling_ratio() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, JsonSchema, clapfig::Schema)]
#[schemars(deny_unknown_fields)]
pub struct Config {
    /// May be omitted; defaults to 0.0.0.0:8080, or override with --listen.
    #[schemars(default = "default_listen")]
    #[clapfig(default = "0.0.0.0:8080")]
    pub listen: String,
    /// May be omitted when provided with --bot-token; the merged value must be non-empty.
    #[schemars(default = "default_bot_token")]
    #[clapfig(default = "")]
    pub bot_token: String,
    /// May be provided with --default-chat-id.
    pub default_chat_id: Option<i64>,
    /// May be omitted; defaults to templates, or override with --templates-dir.
    #[schemars(default = "default_templates_dir")]
    #[clapfig(value, default = "templates")]
    pub templates_dir: PathBuf,
    #[schemars(default = "default_telegram_api_url")]
    #[clapfig(default = "https://api.telegram.org")]
    pub telegram_api_url: String,
    #[clapfig(default = [])]
    pub api_secrets: Vec<String>,
    /// Extra POST routes; each renders a fixed template from the whole request body.
    #[schemars(default)]
    pub endpoints: Vec<EndpointConfig>,
    #[schemars(default)]
    pub otlp: OtlpConfig,
}

/// A custom POST route. The request body is used as the template `values`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, clapfig::Schema)]
#[schemars(deny_unknown_fields)]
pub struct EndpointConfig {
    /// URL path starting with `/`, for example `/alerts`.
    pub path: String,
    /// Template name, without the extension, rendered for every request.
    pub template: String,
    /// Chat id for this endpoint; defaults to default_chat_id when omitted.
    pub chat_id: Option<i64>,
}

const RESERVED_PATHS: [&str; 2] = ["/health", "/notify"];

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, clapfig::Schema)]
#[schemars(deny_unknown_fields)]
pub struct OtlpConfig {
    pub endpoint: Option<String>,
    #[schemars(default = "default_service_name")]
    #[clapfig(default = "bakanotifier")]
    pub service_name: String,
    #[schemars(default = "default_sampling_ratio")]
    #[clapfig(default = 1.0)]
    pub sampling_ratio: f64,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            service_name: default_service_name(),
            sampling_ratio: default_sampling_ratio(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.bot_token.trim().is_empty() {
            bail!("bot_token must not be empty");
        }
        if self.api_secrets.is_empty() {
            bail!("api_secrets must contain at least one secret");
        }
        if self
            .api_secrets
            .iter()
            .any(|secret| secret.trim().is_empty())
        {
            bail!("api_secrets must not contain empty secrets");
        }
        if self.telegram_api_url.trim().is_empty() {
            bail!("telegram_api_url must not be empty");
        }
        if !self.templates_dir.is_dir() {
            bail!(
                "templates_dir {} does not exist or is not a directory",
                self.templates_dir.display()
            );
        }
        self.validate_endpoints()?;
        if self.otlp.service_name.trim().is_empty() {
            bail!("otlp.service_name must not be empty");
        }
        if !(0.0..=1.0).contains(&self.otlp.sampling_ratio) {
            bail!("otlp.sampling_ratio must be between 0.0 and 1.0");
        }
        Ok(())
    }

    fn validate_endpoints(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for endpoint in &self.endpoints {
            let path = endpoint.path.as_str();
            if !path.starts_with('/') || path.len() < 2 {
                bail!("endpoint path {path:?} must start with / and name a segment");
            }
            if path.contains(['{', '}', '*', ':', '?', '#']) || path.ends_with('/') {
                bail!("endpoint path {path:?} must be a literal path without wildcards or query");
            }
            if RESERVED_PATHS.contains(&path) {
                bail!("endpoint path {path:?} is reserved");
            }
            if !seen.insert(path) {
                bail!("endpoint path {path:?} is defined more than once");
            }
            // Same name rule as the runtime template loader; kept here because
            // build.rs compiles this file without the rest of the crate.
            let name_ok = !endpoint.template.is_empty()
                && endpoint
                    .template
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !name_ok {
                bail!(
                    "endpoint {path} template name {:?} must match ^[A-Za-z0-9_-]+$",
                    endpoint.template
                );
            }
            let exists = ["yaml", "yml"].iter().any(|ext| {
                self.templates_dir
                    .join(format!("{}.{ext}", endpoint.template))
                    .is_file()
            });
            if !exists {
                bail!(
                    "endpoint {path} template {:?} not found in {}",
                    endpoint.template,
                    self.templates_dir.display()
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config() -> Config {
        Config {
            listen: "127.0.0.1:8080".to_string(),
            bot_token: "123456:ABC-DEF".to_string(),
            default_chat_id: None,
            templates_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("templates"),
            telegram_api_url: "https://api.telegram.org".to_string(),
            api_secrets: vec!["change-me".to_string()],
            endpoints: Vec::new(),
            otlp: OtlpConfig::default(),
        }
    }

    #[test]
    fn accepts_valid_config() {
        config().validate().expect("config should be valid");
    }

    #[test]
    fn rejects_empty_api_secrets() {
        let mut config = config();
        config.api_secrets.clear();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("api_secrets"));
    }

    #[test]
    fn rejects_empty_bot_token() {
        let mut config = config();
        config.bot_token = "  ".to_string();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("bot_token"));
    }

    #[test]
    fn rejects_missing_templates_dir() {
        let mut config = config();
        config.templates_dir = "/nonexistent/bakanotifier-templates".into();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("templates_dir"));
    }

    fn with_endpoint(path: &str, template: &str) -> Config {
        let mut config = config();
        config.endpoints = vec![EndpointConfig {
            path: path.to_string(),
            template: template.to_string(),
            chat_id: None,
        }];
        config
    }

    #[test]
    fn accepts_valid_endpoint() {
        with_endpoint("/alerts", "text")
            .validate()
            .expect("endpoint should be valid");
    }

    #[test]
    fn rejects_bad_endpoint_paths() {
        for path in ["alerts", "/", "/a/", "/notify", "/health", "/x/{id}", "/a*"] {
            let err = with_endpoint(path, "text").validate().unwrap_err();
            assert!(err.to_string().contains("endpoint path"), "{path}: {err}");
        }
    }

    #[test]
    fn rejects_duplicate_endpoint_paths() {
        let mut config = with_endpoint("/alerts", "text");
        config.endpoints.push(config.endpoints[0].clone());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn rejects_missing_endpoint_template() {
        let err = with_endpoint("/alerts", "nope").validate().unwrap_err();
        assert!(err.to_string().contains("not found"));
        let err = with_endpoint("/alerts", "../etc").validate().unwrap_err();
        assert!(err.to_string().contains("must match"));
    }

    #[test]
    fn rejects_invalid_sampling_ratio() {
        let mut config = config();
        config.otlp.sampling_ratio = 2.0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("sampling_ratio"));
    }
}
