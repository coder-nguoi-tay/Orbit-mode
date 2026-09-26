use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::models::{ProviderQuota, QuotaWindow};

const READ_TIMEOUT: Duration = Duration::from_secs(25);

pub fn fetch_claude_quota(
    executable: &str,
    account_key: &str,
    provider_account_id: Option<&str>,
) -> Result<ProviderQuota, String> {
    let mut child = Command::new(executable)
        .args([
            "-p",
            "1",
            "--output-format",
            "stream-json",
            "--verbose",
            "--max-turns",
            "1",
            "--dangerously-skip-permissions",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start claude: {e}"))?;

    let stdout = child.stdout.take().ok_or("claude stdout unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(scan_for_rate_limit(stdout));
    });

    let outcome = receiver.recv_timeout(READ_TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();

    let info = match outcome {
        Ok(Ok(value)) => value,
        Ok(Err(msg)) => return Err(msg),
        Err(_) => return Err("claude did not emit rate limits in time".into()),
    };

    let five_hour = parse_window(&info, "five_hour");
    let seven_day = parse_window(&info, "seven_day");
    if five_hour.is_none() && seven_day.is_none() {
        return Err("claude reported no rate-limit windows".into());
    }

    Ok(ProviderQuota {
        provider: "claude-code".into(),
        account_key: account_key.to_string(),
        provider_account_id: provider_account_id.map(str::to_owned),
        five_hour,
        seven_day,
        updated_at: chrono::Utc::now().to_rfc3339(),
        source: "cli_probe".into(),
    })
}

fn scan_for_rate_limit(stdout: impl std::io::Read) -> Result<serde_json::Value, String> {
    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines().take(64) {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if msg.get("type").and_then(|v| v.as_str()) == Some("rate_limit_event") {
            if let Some(info) = msg.get("rate_limit_info") {
                return Ok(info.clone());
            }
        }
    }
    Err("claude stream ended without rate_limit_event".into())
}

fn parse_window(info: &serde_json::Value, name: &str) -> Option<QuotaWindow> {
    let window = info.get("unifiedWindows").and_then(|w| w.get(name))?;
    Some(QuotaWindow {
        utilization: window
            .get("utilization")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        resets_at: window
            .get("resetsAt")
            .or_else(|| window.get("resets_at"))
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64))),
        status: info
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_info() -> serde_json::Value {
        serde_json::json!({
            "status": "allowed",
            "unifiedWindows": {
                "five_hour": { "utilization": 0.35, "resetsAt": 1790411400i64 },
                "seven_day": { "utilization": 0.17, "resetsAt": 1790845200i64 }
            }
        })
    }

    #[test]
    fn parses_five_hour() {
        let w = parse_window(&sample_info(), "five_hour").unwrap();
        assert!((w.utilization - 0.35).abs() < f64::EPSILON);
        assert_eq!(w.resets_at, Some(1790411400));
        assert_eq!(w.status.as_deref(), Some("allowed"));
    }

    #[test]
    fn parses_seven_day() {
        let w = parse_window(&sample_info(), "seven_day").unwrap();
        assert!((w.utilization - 0.17).abs() < f64::EPSILON);
    }

    #[test]
    fn scan_finds_rate_limit_event() {
        let stream = concat!(
            "{\"type\":\"system\",\"data\":{}}\n",
            "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed\",\"unifiedWindows\":{\"five_hour\":{\"utilization\":0.1,\"resetsAt\":9999999999}}}}\n",
            "{\"type\":\"result\"}\n"
        );
        let info = scan_for_rate_limit(stream.as_bytes()).unwrap();
        assert_eq!(info["status"], "allowed");
    }

    #[test]
    fn scan_fails_without_event() {
        let stream = "{\"type\":\"system\"}\n{\"type\":\"result\"}\n";
        assert!(scan_for_rate_limit(stream.as_bytes()).is_err());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    fn reads_live_quota_from_installed_cli() {
        let Some(executable) = crate::services::spawn_manager::find_claude() else {
            eprintln!("claude CLI not installed; skipping");
            return;
        };
        match fetch_claude_quota(&executable, "default", None) {
            Ok(quota) => {
                assert_eq!(quota.provider, "claude-code");
                assert_eq!(quota.source, "cli_probe");
                let five = quota.five_hour.expect("no 5-hour window");
                assert!(
                    (0.0..=1.0).contains(&five.utilization),
                    "out of range: {}",
                    five.utilization
                );
                eprintln!(
                    "live 5h={:.0}% 7d={:?}",
                    five.utilization * 100.0,
                    quota.seven_day.map(|w| w.utilization)
                );
            }
            Err(e) => eprintln!("live read unavailable ({e}); skipping"),
        }
    }
}
