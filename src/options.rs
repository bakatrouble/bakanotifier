use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use clapfig::{Clapfig, Layer, SearchPath};
use serde::Serialize;

use crate::config::Config;

#[derive(Debug, Parser, Serialize)]
#[command(about = "HTTP service that sends notifications to Telegram")]
pub struct Opts {
    #[clap(subcommand)]
    #[serde(skip)]
    command: Option<Command>,

    /// Path to the TOML configuration file
    #[arg(long, default_value = "config.toml")]
    #[serde(skip)]
    config: PathBuf,

    /// Address to bind (default 0.0.0.0:8080)
    #[arg(long)]
    listen: Option<String>,

    /// Telegram bot token (secret; prefer the environment or TOML file)
    #[arg(long, value_name = "TOKEN", hide_default_value = true)]
    bot_token: Option<String>,

    /// Default chat id used when /notify omits chat_id
    #[arg(long, value_parser = parse_chat_id)]
    default_chat_id: Option<i64>,

    /// Directory containing rich YAML templates (default templates)
    #[arg(long)]
    templates_dir: Option<PathBuf>,

    /// Telegram Bot API base URL (default https://api.telegram.org)
    #[arg(long)]
    telegram_api_url: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the local HTTP health endpoint and exit non-zero when unhealthy
    Healthcheck(HealthcheckArgs),
}

#[derive(Debug, Args)]
pub struct HealthcheckArgs {
    /// Address of the local bakanotifier listener
    #[arg(long, default_value = "127.0.0.1:8080")]
    address: String,
}

fn parse_chat_id(value: &str) -> Result<i64, String> {
    value
        .parse()
        .map_err(|_| "expected an integer for BN__DEFAULT_CHAT_ID or --default-chat-id".to_string())
}

impl Opts {
    pub fn config_path(&self) -> &Path {
        &self.config
    }
}

pub fn load_config(path: &Path, cli: &impl Serialize) -> Result<Config> {
    load_config_inner(path, cli, true)
}

pub fn load_config_unvalidated(path: &Path, cli: &impl Serialize) -> Result<Config> {
    load_config_inner(path, cli, false)
}

fn load_config_inner(path: &Path, cli: &impl Serialize, validate: bool) -> Result<Config> {
    let (directory, file_name) = config_location(path)?;
    let mut builder = Clapfig::typed::<Config>()
        .app_name("bakanotifier")
        .file_name(&file_name)
        .search_paths(vec![SearchPath::Path(directory)])
        .env_prefix("BN")
        .strict(true)
        .layer_order(vec![Layer::Files, Layer::Env, Layer::Cli])
        .cli_overrides_from(cli);

    if validate {
        builder =
            builder.post_validate(|config| config.validate().map_err(|error| error.to_string()));
    }

    builder
        .load()
        .with_context(|| format!("loading config {}", path.display()))
}

fn config_location(path: &Path) -> Result<(PathBuf, String)> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("config path {} must name a file", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = std::fs::canonicalize(parent)
        .with_context(|| format!("resolving config directory {}", parent.display()))?;
    Ok((directory, file_name.to_string()))
}

pub fn preview_config(path: &Path, cli: &impl Serialize) -> Result<Config> {
    load_config_unvalidated(path, cli)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, FromArgMatches};
    use std::fs;

    fn temp_config(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bakanotifier-clapfig-test-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("config.toml");
        fs::write(&path, contents).expect("config should be written");
        path
    }

    fn base_toml(extra: &str) -> String {
        format!(
            "templates_dir = \"{}\"\napi_secrets = [\"change-me\"]\n{extra}",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("templates")
                .display()
        )
    }

    fn clean_env<R>(f: impl FnOnce() -> R) -> R {
        temp_env::with_vars_unset(
            [
                "BN__LISTEN",
                "BN__BOT_TOKEN",
                "BN__DEFAULT_CHAT_ID",
                "BN__TEMPLATES_DIR",
                "BN__TELEGRAM_API_URL",
                "BN__OTLP__ENDPOINT",
                "BN__OTLP__SERVICE_NAME",
                "BN__OTLP__SAMPLING_RATIO",
            ],
            f,
        )
    }

    fn load_with_env(path: &Path, args: &[&str], envs: &[(&str, Option<&str>)]) -> Config {
        temp_env::with_vars(envs, || {
            let matches = Opts::command()
                .try_get_matches_from(args)
                .expect("arguments should parse");
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            load_config_unvalidated(path, &opts).expect("config should load")
        })
    }

    #[test]
    fn listen_and_bot_token_precedence() {
        let path = temp_config(
            "precedence",
            &base_toml("listen = \"127.0.0.1:1111\"\nbot_token = \"toml:token\"\n"),
        );

        let defaults = load_with_env(
            &temp_config("defaults", &base_toml("")),
            &["bakanotifier"],
            &[("BN__LISTEN", None), ("BN__BOT_TOKEN", None)],
        );
        assert_eq!(defaults.listen, "0.0.0.0:8080");
        assert_eq!(defaults.bot_token, "");

        let toml = load_with_env(
            &path,
            &["bakanotifier"],
            &[("BN__LISTEN", None), ("BN__BOT_TOKEN", None)],
        );
        assert_eq!(toml.listen, "127.0.0.1:1111");
        assert_eq!(toml.bot_token, "toml:token");

        let env = load_with_env(
            &path,
            &["bakanotifier"],
            &[
                ("BN__LISTEN", Some("127.0.0.1:2222")),
                ("BN__BOT_TOKEN", Some("env:token")),
            ],
        );
        assert_eq!(env.listen, "127.0.0.1:2222");
        assert_eq!(env.bot_token, "env:token");

        let cli = load_with_env(
            &path,
            &[
                "bakanotifier",
                "--listen",
                "127.0.0.1:3333",
                "--bot-token",
                "cli:token",
            ],
            &[
                ("BN__LISTEN", Some("127.0.0.1:2222")),
                ("BN__BOT_TOKEN", Some("env:token")),
            ],
        );
        assert_eq!(cli.listen, "127.0.0.1:3333");
        assert_eq!(cli.bot_token, "cli:token");
    }

    #[test]
    fn bare_listen_is_not_read() {
        let path = temp_config(
            "bare-listen",
            &base_toml("listen = \"127.0.0.1:1111\"\nbot_token = \"toml:token\"\n"),
        );
        let config = load_with_env(
            &path,
            &["bakanotifier"],
            &[("LISTEN", Some("127.0.0.1:9999"))],
        );
        assert_eq!(config.listen, "127.0.0.1:1111");
    }

    #[test]
    fn bad_integer_environment_value_is_rejected() {
        let path = temp_config(
            "bad-chat-id",
            &base_toml("bot_token = \"123456:ABC-DEF\"\n"),
        );
        temp_env::with_vars([("BN__DEFAULT_CHAT_ID", Some("abc"))], || {
            let matches = Opts::command().get_matches_from(["bakanotifier"]);
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            let error =
                load_config_unvalidated(&path, &opts).expect_err("invalid env integer should fail");
            let message = format!("{error:#}");
            assert!(
                message.contains("DEFAULT_CHAT_ID") || message.contains("default_chat_id"),
                "unclear error: {message}"
            );
        });
    }

    #[test]
    fn unknown_toml_key_is_rejected_with_path_and_key() {
        let path = temp_config("unknown", &base_toml("unknown_key = true\n"));
        clean_env(|| {
            let matches = Opts::command().get_matches_from(["bakanotifier"]);
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            let error = load_config_unvalidated(&path, &opts).expect_err("unknown key should fail");
            let message = format!("{error:#}");
            assert!(message.contains("unknown_key"), "{message}");
            assert!(message.contains(path.to_str().unwrap()), "{message}");
        });
    }

    #[test]
    fn toml_otlp_section_loads() {
        let path = temp_config(
            "otlp",
            &base_toml("[otlp]\nendpoint = \"http://collector:4318\"\nsampling_ratio = 0.25\n"),
        );
        let config = clean_env(|| {
            let matches = Opts::command().get_matches_from(["bakanotifier"]);
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            load_config_unvalidated(&path, &opts).expect("otlp config should load")
        });
        assert_eq!(
            config.otlp.endpoint.as_deref(),
            Some("http://collector:4318")
        );
        assert!((config.otlp.sampling_ratio - 0.25).abs() < f64::EPSILON);
        assert_eq!(config.otlp.service_name, "bakanotifier");
    }

    #[test]
    fn nested_otlp_environment_value_overrides_toml() {
        let path = temp_config(
            "otlp-env",
            &base_toml("bot_token = \"123456:ABC-DEF\"\n[otlp]\nendpoint = \"http://toml:4318\"\n"),
        );
        let config = load_with_env(
            &path,
            &["bakanotifier"],
            &[("BN__OTLP__ENDPOINT", Some("http://env:4318"))],
        );
        assert_eq!(config.otlp.endpoint.as_deref(), Some("http://env:4318"));
    }

    #[test]
    fn missing_api_secrets_is_rejected() {
        let path = temp_config(
            "missing-secrets",
            &format!(
                "templates_dir = \"{}\"\nbot_token = \"123456:ABC-DEF\"\n",
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("templates")
                    .display()
            ),
        );
        clean_env(|| {
            let matches = Opts::command().get_matches_from(["bakanotifier"]);
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            let error = load_config(&path, &opts).expect_err("api_secrets should be required");
            assert!(format!("{error:#}").contains("api_secrets"), "{error:#}");
        });
    }

    #[test]
    fn config_example_toml_loads_and_validates() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
        let config = clean_env(|| {
            let matches = Opts::command().get_matches_from(["bakanotifier"]);
            let opts = Opts::from_arg_matches(&matches).expect("options should parse");
            load_config(&path, &opts).expect("example should load")
        });
        assert_eq!(config.api_secrets, vec!["change-me".to_string()]);
    }
}
