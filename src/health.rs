use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::telegram::TelegramClient;

pub const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Result of the most recent Bot API check, shared between the poller and the
/// HTTP handler.
#[derive(Debug, Default)]
pub struct BotHealth {
    last_checked: Option<u64>,
    last_ok: Option<u64>,
    latency_ms: Option<u64>,
    last_error: Option<String>,
}

pub type SharedHealth = Arc<Mutex<BotHealth>>;

#[must_use]
pub fn shared_health() -> SharedHealth {
    Arc::new(Mutex::new(BotHealth::default()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

/// Calls getMe immediately and then every HEALTH_CHECK_INTERVAL, recording the
/// outcome for the health endpoint. Stops when shutdown changes or is dropped.
pub async fn poll_bot_health(
    telegram: TelegramClient,
    health: SharedHealth,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let started = Instant::now();
        let result = tokio::select! {
            result = telegram.check_bot() => result,
            _ = shutdown.changed() => break,
        };
        let now = unix_now();
        {
            let mut health = health.lock().expect("health state lock");
            health.last_checked = Some(now);
            match result {
                Ok(()) => {
                    health.last_ok = Some(now);
                    health.latency_ms =
                        Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
                    health.last_error = None;
                }
                Err(error) => {
                    health.latency_ms = None;
                    health.last_error = Some(error.message.clone());
                    tracing::error!(error = %error, "telegram health check failed");
                }
            }
        }

        tokio::select! {
            () = tokio::time::sleep(HEALTH_CHECK_INTERVAL) => {},
            _ = shutdown.changed() => break,
        }
    }
}

/// Builds the status code and JSON body from the last recorded check.
///
/// "ok" needs the latest check to have succeeded and that success to be
/// recent, so a failed check reads as down at once and a stalled poller reads
/// as down too.
#[must_use]
pub fn health_response(health: &BotHealth) -> (StatusCode, Value) {
    let fresh = health.last_error.is_none()
        && health.last_ok.is_some_and(|time| {
            unix_now().saturating_sub(time) <= 2 * HEALTH_CHECK_INTERVAL.as_secs()
        });
    let status = if fresh {
        "ok"
    } else if health.last_checked.is_none() {
        "starting"
    } else {
        "down"
    };
    let code = if fresh {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = json!({
        "status": status,
        "telegram": {
            "reachable": health.last_checked.is_some() && health.last_error.is_none(),
            "last_checked_unix": health.last_checked,
            "last_ok_unix": health.last_ok,
            "latency_ms": health.latency_ms,
            "last_error": health.last_error,
        }
    });
    (code, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_minus(seconds: u64) -> u64 {
        unix_now().saturating_sub(seconds)
    }

    #[test]
    fn starting_before_first_check() {
        let (status, body) = health_response(&BotHealth::default());
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "starting");
        assert_eq!(body["telegram"]["reachable"], false);
        assert!(body["telegram"]["last_checked_unix"].is_null());
        assert!(body["telegram"]["last_ok_unix"].is_null());
    }

    #[test]
    fn ok_after_recent_success() {
        let health = BotHealth {
            last_checked: Some(now_minus(1)),
            last_ok: Some(now_minus(1)),
            latency_ms: Some(12),
            last_error: None,
        };
        let (status, body) = health_response(&health);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["telegram"]["reachable"], true);
        assert_eq!(body["telegram"]["latency_ms"], 12);
        assert!(body["telegram"]["last_error"].is_null());
    }

    #[test]
    fn down_after_failure() {
        let health = BotHealth {
            last_checked: Some(now_minus(1)),
            last_ok: Some(now_minus(10)),
            latency_ms: None,
            last_error: Some("Unauthorized".to_string()),
        };
        let (status, body) = health_response(&health);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "down");
        assert_eq!(body["telegram"]["reachable"], false);
        assert_eq!(body["telegram"]["last_error"], "Unauthorized");
    }

    #[test]
    fn down_when_last_success_is_stale() {
        let stale = 2 * HEALTH_CHECK_INTERVAL.as_secs() + 5;
        let health = BotHealth {
            last_checked: Some(now_minus(stale)),
            last_ok: Some(now_minus(stale)),
            latency_ms: Some(5),
            last_error: None,
        };
        let (status, body) = health_response(&health);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "down");
        assert_eq!(body["telegram"]["reachable"], true);
    }
}
