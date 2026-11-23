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
    #[schemars(default)]
    pub otlp: OtlpConfig,
}

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
        if self.otlp.service_name.trim().is_empty() {
            bail!("otlp.service_name must not be empty");
        }
        if !(0.0..=1.0).contains(&self.otlp.sampling_ratio) {
            bail!("otlp.sampling_ratio must be between 0.0 and 1.0");
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

    #[test]
    fn rejects_invalid_sampling_ratio() {
        let mut config = config();
        config.otlp.sampling_ratio = 2.0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("sampling_ratio"));
    }
}
