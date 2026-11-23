use std::time::Duration;

use anyhow::Context;
use clap::{CommandFactory, FromArgMatches};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use bakanotifier::api::build_app_with_health;
use bakanotifier::health::{poll_bot_health, shared_health};
use bakanotifier::options::{Opts, load_config};
use bakanotifier::telegram::TelegramClient;
use bakanotifier::telemetry::Telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = <Opts as CommandFactory>::command().get_matches();

    if let Some(("healthcheck", subcommand)) = matches.subcommand() {
        let address = subcommand
            .get_one::<String>("address")
            .map_or("127.0.0.1:8080", String::as_str);
        return run_healthcheck(address).await;
    }

    let opts = Opts::from_arg_matches(&matches)?;
    let config = load_config(opts.config_path(), &opts)?;
    let telemetry = Telemetry::init(&config)?;
    let listen = config.listen.clone();
    let health = shared_health();
    let telegram = TelegramClient::from_config(&config);
    let (health_shutdown, health_shutdown_rx) = tokio::sync::watch::channel(false);

    tracing::info!(
        listen = %listen,
        templates_dir = %config.templates_dir.display(),
        "starting bakanotifier"
    );

    let app = build_app_with_health(config, health.clone());
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    let poller = tokio::spawn(poll_bot_health(telegram, health, health_shutdown_rx));

    let shutdown = {
        let health_shutdown = health_shutdown.clone();
        async move {
            shutdown_signal().await;
            let _ = health_shutdown.send(true);
        }
    };
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .context("running HTTP server");

    let _ = health_shutdown.send(true);
    let _ = poller.await;
    telemetry.shutdown();
    result
}

async fn run_healthcheck(address: &str) -> anyhow::Result<()> {
    let check = async {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .with_context(|| format!("connecting to {address}"))?;
        let request =
            format!("GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .context("writing health request")?;
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .context("reading health response")?;
        let status_line = response.lines().next().unwrap_or_default();
        let status = status_line.split_whitespace().nth(1).unwrap_or_default();
        if status == "200" {
            Ok(())
        } else {
            anyhow::bail!("health endpoint returned {status_line:?}")
        }
    };

    tokio::time::timeout(Duration::from_secs(5), check)
        .await
        .context("health check timed out")?
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to listen for ctrl_c");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
