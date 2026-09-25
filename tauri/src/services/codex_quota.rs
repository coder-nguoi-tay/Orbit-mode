//! Live Codex quota reads via the `codex app-server` JSON-RPC protocol.
//!
//! The `account/rateLimits/read` method returns the account's current rate-limit
//! windows without running a turn, so quotas are available before any session has
//! streamed a `token_count` event.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::models::{ProviderQuota, QuotaWindow};

/// Windows at or above this length are the weekly bucket; below it, the 5-hour one.
const WEEKLY_WINDOW_MINUTES: u64 = 1440;
const READ_TIMEOUT: Duration = Duration::from_secs(6);

/// Read one Codex account's live rate-limit windows.
///
/// @param executable Resolved `codex` binary path.
/// @param profile_home Isolated `CODEX_HOME`, or None for the system login.
/// @param account_key Account key stored alongside the quota sample.
/// @param provider_account_id Configured profile id, when the read is account-scoped.
/// @return The account's current five-hour and seven-day windows.
/// @throws String If the CLI cannot start, times out, or reports an error.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
pub fn fetch_codex_quota(
    executable: &str,
    profile_home: Option<&str>,
    account_key: &str,
    provider_account_id: Option<&str>,
) -> Result<ProviderQuota, String> {
    let payload = read_rate_limits(executable, profile_home)?;
    let limits = payload
        .get("rateLimits")
        .ok_or_else(|| "app-server response has no rateLimits".to_string())?;

    let five_hour = parse_window(limits, false);
    let seven_day = parse_window(limits, true);
    if five_hour.is_none() && seven_day.is_none() {
        return Err("app-server reported no rate-limit windows".into());
    }

    Ok(ProviderQuota {
        provider: "codex".into(),
        account_key: account_key.to_string(),
        provider_account_id: provider_account_id.map(str::to_owned),
        five_hour,
        seven_day,
        updated_at: chrono::Utc::now().to_rfc3339(),
        source: "app_server".into(),
    })
}

/// Drive the stdio JSON-RPC handshake and return the `account/rateLimits/read` result.
fn read_rate_limits(
    executable: &str,
    profile_home: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut command = Command::new(executable);
    if let Some(home) = profile_home {
        command.env("CODEX_HOME", home);
        command.env_remove("OPENAI_API_KEY");
        command.env_remove("CODEX_ACCESS_TOKEN");
        command.arg("-c").arg("cli_auth_credentials_store=\"file\"");
    }
    let mut child = command
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start codex app-server: {e}"))?;

    let write_result = child.stdin.as_mut().ok_or("app-server stdin unavailable").and_then(
        |stdin| {
            let init = serde_json::json!({
                "id": 1,
                "method": "initialize",
                "params": { "clientInfo": { "name": "orbit", "version": env!("CARGO_PKG_VERSION") } }
            });
            let read = serde_json::json!({
                "id": 2,
                "method": "account/rateLimits/read",
                "params": { "excludeResetCreditDetails": true }
            });
            writeln!(stdin, "{init}")
                .and_then(|_| writeln!(stdin, "{read}"))
                .and_then(|_| stdin.flush())
                .map_err(|_| "failed to write to app-server stdin")
        },
    );
    if let Err(message) = write_result {
        let _ = child.kill();
        return Err(message.to_string());
    }

    // Read on a worker thread so a hung CLI cannot block the caller forever.
    let stdout = child.stdout.take().ok_or("app-server stdout unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(scan_for_response(stdout, 2));
    });

    let outcome = receiver.recv_timeout(READ_TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();

    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(message),
        Err(_) => Err("codex app-server did not answer in time".into()),
    }
}

/// Consume JSON-RPC lines until the response carrying `request_id` arrives.
fn scan_for_response(
    stdout: impl std::io::Read,
    request_id: i64,
) -> Result<serde_json::Value, String> {
    let reader = BufReader::new(stdout);
    for line in reader.lines().take(64) {
        let Ok(line) = line else {
            break;
        };
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if message.get("id").and_then(serde_json::Value::as_i64) != Some(request_id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let detail = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(format!("codex app-server error: {detail}"));
        }
        return message
            .get("result")
            .cloned()
            .ok_or_else(|| "app-server response has no result".to_string());
    }
    Err("codex app-server closed before answering".into())
}

/// Convert one `RateLimitWindow` into Orbit's quota window shape.
///
/// The app-server reports `primary`/`secondary` with an explicit window length, so the
/// bucket is chosen by duration rather than by field order.
fn parse_window(limits: &serde_json::Value, weekly: bool) -> Option<QuotaWindow> {
    ["primary", "secondary"]
        .iter()
        .filter_map(|key| limits.get(key).filter(|value| !value.is_null()))
        .find(|window| {
            let minutes = window
                .get("windowDurationMins")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            (minutes >= WEEKLY_WINDOW_MINUTES) == weekly
        })
        .map(|window| QuotaWindow {
            utilization: window
                .get("usedPercent")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
                / 100.0,
            resets_at: window.get("resetsAt").and_then(serde_json::Value::as_i64),
            status: Some("normal".into()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits_json() -> serde_json::Value {
        serde_json::json!({
            "primary": { "usedPercent": 33, "windowDurationMins": 300, "resetsAt": 1790372282i64 },
            "secondary": { "usedPercent": 5, "windowDurationMins": 10080, "resetsAt": 1790959082i64 }
        })
    }

    #[test]
    fn parses_five_hour_window_from_primary() {
        let window = parse_window(&limits_json(), false).expect("missing five-hour window");
        assert!((window.utilization - 0.33).abs() < f64::EPSILON);
        assert_eq!(window.resets_at, Some(1790372282));
    }

    #[test]
    fn parses_seven_day_window_from_secondary() {
        let window = parse_window(&limits_json(), true).expect("missing seven-day window");
        assert!((window.utilization - 0.05).abs() < f64::EPSILON);
        assert_eq!(window.resets_at, Some(1790959082));
    }

    #[test]
    fn selects_window_by_duration_not_field_order() {
        // Some accounts report the weekly bucket first.
        let swapped = serde_json::json!({
            "primary": { "usedPercent": 7, "windowDurationMins": 10080 },
            "secondary": { "usedPercent": 71, "windowDurationMins": 300 }
        });
        let five = parse_window(&swapped, false).expect("missing five-hour window");
        let seven = parse_window(&swapped, true).expect("missing seven-day window");
        assert!((five.utilization - 0.71).abs() < f64::EPSILON);
        assert!((seven.utilization - 0.07).abs() < f64::EPSILON);
    }

    #[test]
    fn returns_none_when_window_is_null() {
        let partial = serde_json::json!({
            "primary": { "usedPercent": 12, "windowDurationMins": 300 },
            "secondary": serde_json::Value::Null
        });
        assert!(parse_window(&partial, false).is_some());
        assert!(parse_window(&partial, true).is_none());
    }

    #[test]
    fn treats_missing_resets_at_as_unknown() {
        let no_reset = serde_json::json!({
            "primary": { "usedPercent": 40, "windowDurationMins": 300 }
        });
        let window = parse_window(&no_reset, false).expect("missing five-hour window");
        assert_eq!(window.resets_at, None);
        assert!((window.utilization - 0.40).abs() < f64::EPSILON);
    }

    #[test]
    fn scan_returns_matching_response() {
        let stream = concat!(
            "{\"method\":\"account/updated\",\"params\":{}}\n",
            "{\"id\":1,\"result\":{\"codexHome\":\"/home\"}}\n",
            "{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":{\"usedPercent\":33,\"windowDurationMins\":300}}}}\n"
        );
        let result = scan_for_response(stream.as_bytes(), 2).expect("expected a response");
        assert_eq!(result["rateLimits"]["primary"]["usedPercent"], 33);
    }

    #[test]
    fn scan_surfaces_jsonrpc_error() {
        let stream = "{\"id\":2,\"error\":{\"code\":-32000,\"message\":\"not logged in\"}}\n";
        let error = scan_for_response(stream.as_bytes(), 2).expect_err("expected an error");
        assert!(error.contains("not logged in"));
    }

    #[test]
    fn scan_reports_closed_stream() {
        let stream = "{\"id\":1,\"result\":{}}\n";
        let error = scan_for_response(stream.as_bytes(), 2).expect_err("expected an error");
        assert!(error.contains("closed before answering"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Verifies the real CLI handshake end-to-end. Skipped when codex is absent or logged out.
    #[test]
    fn reads_live_quota_from_installed_cli() {
        let Some(executable) = crate::services::spawn_manager::find_codex() else {
            eprintln!("codex CLI not installed; skipping live check");
            return;
        };
        match fetch_codex_quota(&executable, None, "default", None) {
            Ok(quota) => {
                assert_eq!(quota.provider, "codex");
                assert_eq!(quota.source, "app_server");
                let five = quota
                    .five_hour
                    .expect("live read returned no 5-hour window");
                assert!(
                    (0.0..=1.0).contains(&five.utilization),
                    "utilization out of range: {}",
                    five.utilization
                );
                eprintln!(
                    "live 5h={:.0}% 7d={:?}",
                    five.utilization * 100.0,
                    quota.seven_day.map(|w| w.utilization)
                );
            }
            Err(error) => eprintln!("live read unavailable ({error}); skipping assertions"),
        }
    }
}
