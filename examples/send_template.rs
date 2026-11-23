use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use serde_json::json;

use bakanotifier::options::{load_config, preview_config};
use bakanotifier::parsing::parse_values_argument;
use bakanotifier::telegram::TelegramClient;
use bakanotifier::template::load_and_render;

#[derive(Debug, Parser, Serialize)]
#[command(about = "Render a rich template locally and optionally send it to Telegram")]
struct Args {
    /// Path to the TOML configuration file
    #[arg(long, default_value = "config.toml")]
    #[serde(skip)]
    config: PathBuf,

    /// Chat id; overrides default_chat_id from the config
    #[arg(long)]
    #[serde(rename = "default_chat_id")]
    chat_id: Option<i64>,

    /// Directory containing templates; overrides templates_dir from the config
    #[arg(long)]
    templates_dir: Option<PathBuf>,

    /// Template name, without the .yaml/.yml extension
    #[arg(long)]
    #[serde(skip)]
    template: String,

    /// Path to a JSON/YAML values file
    #[arg(long, default_value = "{}")]
    #[serde(skip)]
    values: String,

    /// Render and print the Telegram payload without contacting Telegram
    #[arg(long)]
    #[serde(skip)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let values = parse_values_argument(&args.values).map_err(anyhow::Error::msg)?;

    let config = if args.dry_run {
        preview_config(&args.config, &args)
            .with_context(|| format!("loading config {}", args.config.display()))?
    } else {
        load_config(&args.config, &args)
            .with_context(|| format!("loading config {}", args.config.display()))?
    };
    let templates_dir = config.templates_dir.clone();
    let chat_id = config.default_chat_id;

    let template = load_and_render(&templates_dir, &args.template, &values)
        .with_context(|| format!("rendering template {}", args.template))?;
    let rendered = template
        .into_rendered_message()
        .with_context(|| format!("preparing template {}", args.template))?;

    if args.dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&rendered.payload(chat_id))?
        );
        return Ok(());
    }

    let chat_id = chat_id.context("chat_id is required: pass --chat-id or set default_chat_id")?;

    let telegram = TelegramClient::from_config(&config);
    let message = telegram.send_rendered(chat_id, rendered).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "message_id": message.message_id,
            "chat_id": message.chat.id,
        }))?
    );
    Ok(())
}
