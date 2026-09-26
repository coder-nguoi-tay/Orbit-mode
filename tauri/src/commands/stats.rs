#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageStats {
    pub weekly_tokens: u64,
    pub today_tokens: u64,
    pub weekly_messages: u64,
    pub today_messages: u64,
}

#[tauri::command]
pub fn get_claude_usage_stats() -> ClaudeUsageStats {
    let empty = ClaudeUsageStats {
        weekly_tokens: 0,
        today_tokens: 0,
        weekly_messages: 0,
        today_messages: 0,
    };

    let stats_path = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("stats-cache.json"),
        None => return empty,
    };

    let content = match std::fs::read_to_string(&stats_path) {
        Ok(c) => c,
        Err(_) => return empty,
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return empty,
    };

    // Compute today and 7-days-ago in YYYY-MM-DD format using std only
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day: u64 = 86400;
    let today_days = now / secs_per_day;
    let today = days_to_date(today_days);
    let week_start = days_to_date(today_days.saturating_sub(6));

    let mut weekly_tokens: u64 = 0;
    let mut today_tokens: u64 = 0;
    let mut weekly_messages: u64 = 0;
    let mut today_messages: u64 = 0;

    if let Some(arr) = json.get("dailyModelTokens").and_then(|v| v.as_array()) {
        for entry in arr {
            let date = entry.get("date").and_then(|d| d.as_str()).unwrap_or("");
            if date >= week_start.as_str() && date <= today.as_str() {
                if let Some(by_model) = entry.get("tokensByModel").and_then(|v| v.as_object()) {
                    let total: u64 = by_model.values().filter_map(|v| v.as_u64()).sum();
                    weekly_tokens += total;
                    if date == today.as_str() {
                        today_tokens = total;
                    }
                }
            }
        }
    }

    if let Some(arr) = json.get("dailyActivity").and_then(|v| v.as_array()) {
        for entry in arr {
            let date = entry.get("date").and_then(|d| d.as_str()).unwrap_or("");
            if date >= week_start.as_str() && date <= today.as_str() {
                let msgs = entry
                    .get("messageCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                weekly_messages += msgs;
                if date == today.as_str() {
                    today_messages = msgs;
                }
            }
        }
    }

    ClaudeUsageStats {
        weekly_tokens,
        today_tokens,
        weekly_messages,
        today_messages,
    }
}

fn days_to_date(days: u64) -> String {
    // Compute YYYY-MM-DD from days since Unix epoch (1970-01-01)
    let mut remaining = days;
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    let day = remaining + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Raw format from ~/.orbit/status/{pid}.json (snake_case)
#[derive(serde::Deserialize, Default)]
struct RateLimitsRaw {
    #[serde(default)]
    cost: f64,
    #[serde(default)]
    five_hour_pct: f64,
    #[serde(default)]
    five_hour_reset: i64,
    #[serde(default)]
    seven_day_pct: f64,
    #[serde(default)]
    seven_day_reset: i64,
    #[serde(default)]
    context_pct: f64,
}

/// Frontend format (camelCase)
#[derive(serde::Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RateLimits {
    pub cost: f64,
    pub five_hour_pct: f64,
    pub five_hour_reset: i64,
    pub seven_day_pct: f64,
    pub seven_day_reset: i64,
    pub context_pct: f64,
}

impl From<RateLimitsRaw> for RateLimits {
    fn from(r: RateLimitsRaw) -> Self {
        RateLimits {
            cost: r.cost,
            five_hour_pct: r.five_hour_pct,
            five_hour_reset: r.five_hour_reset,
            seven_day_pct: r.seven_day_pct,
            seven_day_reset: r.seven_day_reset,
            context_pct: r.context_pct,
        }
    }
}

/// Read rate-limit data captured by the statusline hook for a given PID.
/// Falls back to PID 1 (global) if the session PID file doesn't exist.
#[tauri::command]
pub fn get_rate_limits(pid: Option<i32>) -> RateLimits {
    let status_dir = match dirs::home_dir() {
        Some(h) => h.join(".orbit").join("status"),
        None => return RateLimits::default(),
    };

    // Try session PID first, then most recently modified file in status dir
    if let Some(p) = pid {
        let path = status_dir.join(format!("{p}.json"));
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(raw) = serde_json::from_str::<RateLimitsRaw>(&content) {
                return raw.into();
            }
        }
    }

    // Fallback: pick the most recently modified .json in status dir
    if let Ok(entries) = std::fs::read_dir(&status_dir) {
        let mut newest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(meta) = path.metadata() {
                    let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    if newest.as_ref().is_none_or(|(_, t)| modified > *t) {
                        newest = Some((path, modified));
                    }
                }
            }
        }
        if let Some((path, _)) = newest {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(raw) = serde_json::from_str::<RateLimitsRaw>(&content) {
                    return raw.into();
                }
            }
        }
    }

    RateLimits::default()
}

#[tauri::command]
pub fn get_changelog() -> String {
    include_str!("../../../CHANGELOG.md").to_string()
}

/// Build the usage overview after replaying persisted provider quota events.
///
/// @param state Session manager containing the database and journal cache.
/// @return Aggregated usage totals, provider quotas and breakdowns.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
#[tauri::command]
pub fn get_usage_overview(
    state: tauri::State<crate::ipc::session::SessionState>,
) -> crate::models::UsageOverview {
    let db = {
        let mut session_manager = state.write();
        session_manager.restore_from_db();
        std::sync::Arc::clone(&session_manager.db)
    };
    db.get_usage_overview()
        .unwrap_or(crate::models::UsageOverview {
            total_tokens_today: 0,
            total_cost_today: 0.0,
            active_agents_count: 0,
            total_sessions_count: 0,
            quotas: vec![],
            project_summaries: vec![],
            model_summaries: vec![],
            token_usage_history: vec![],
        })
}

/// Return provider quotas after refreshing samples held by active CLI sessions.
///
/// @param state Session manager containing the active CLI state and database.
/// @return Latest known provider quotas.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
#[tauri::command]
pub fn get_provider_quotas(
    state: tauri::State<crate::ipc::session::SessionState>,
) -> Vec<crate::models::ProviderQuota> {
    let db = {
        let mut session_manager = state.write();
        session_manager.restore_from_db();
        std::sync::Arc::clone(&session_manager.db)
    };
    db.get_latest_provider_quotas().unwrap_or_default()
}

#[tauri::command]
pub fn get_session_usages(
    state: tauri::State<crate::ipc::session::SessionState>,
    limit: Option<usize>,
) -> Vec<crate::models::SessionUsageSnapshot> {
    let db = std::sync::Arc::clone(&state.read().db);
    db.get_session_usages(limit.unwrap_or(50))
        .unwrap_or_default()
}

/// Read every configured Codex account's live quota straight from the CLI.
///
/// Quota samples otherwise only arrive once a session streams a `token_count` event, so
/// this poll keeps the usage center accurate before any agent has run.
///
/// @param state The session manager holding the shared database.
/// @param app Event emitter for `provider:quota-updated`.
/// @return Freshly read quotas for the accounts that answered.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
/// Spawning the CLI blocks, so this must stay `async` + `spawn_blocking`: a synchronous
/// command would stall Tauri's main thread and freeze the whole window while it waits.
#[tauri::command]
pub async fn refresh_codex_quotas(
    state: tauri::State<'_, crate::ipc::session::SessionState>,
    app: tauri::AppHandle,
) -> Result<Vec<crate::models::ProviderQuota>, crate::ipc::IpcError> {
    use tauri::Emitter;

    let db = std::sync::Arc::clone(&state.read().db);

    let quotas = tauri::async_runtime::spawn_blocking(move || {
        let Some(executable) = crate::services::spawn_manager::find_codex() else {
            return Vec::new();
        };

        // Accounts with an isolated home are read separately; otherwise read the system login.
        let accounts: Vec<(String, Option<String>, Option<String>)> = db
            .list_provider_accounts()
            .unwrap_or_default()
            .into_iter()
            .filter(|account| account.provider_id == "codex")
            .filter(|account| account.auth_type != crate::models::AccountAuthType::ApiKey)
            .map(|account| (account.id.clone(), account.profile_home, Some(account.id)))
            .collect();
        let targets = if accounts.is_empty() {
            vec![("default".to_string(), None, None)]
        } else {
            accounts
        };

        targets
            .into_iter()
            .filter_map(|(account_key, profile_home, account_id)| {
                match crate::services::codex_quota::fetch_codex_quota(
                    &executable,
                    profile_home.as_deref(),
                    &account_key,
                    account_id.as_deref(),
                ) {
                    Ok(quota) => {
                        let _ = db.record_provider_quota(&quota);
                        Some(quota)
                    }
                    Err(error) => {
                        eprintln!("[orbit:quota] codex read failed for {account_key}: {error}");
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| crate::ipc::IpcError::Other(format!("quota read task failed: {e}")))?;

    for quota in &quotas {
        let _ = app.emit("provider:quota-updated", quota);
    }
    Ok(quotas)
}

#[tauri::command]
pub async fn refresh_claude_quotas(
    state: tauri::State<'_, crate::ipc::session::SessionState>,
    app: tauri::AppHandle,
) -> Result<Vec<crate::models::ProviderQuota>, crate::ipc::IpcError> {
    use tauri::Emitter;

    let db = std::sync::Arc::clone(&state.read().db);

    let quotas = tauri::async_runtime::spawn_blocking(move || {
        let Some(executable) = crate::services::spawn_manager::find_claude() else {
            return Vec::new();
        };

        match crate::services::claude_quota::fetch_claude_quota(&executable, "default", None) {
            Ok(quota) => {
                let _ = db.record_provider_quota(&quota);
                vec![quota]
            }
            Err(error) => {
                eprintln!("[orbit:quota] claude read failed: {error}");
                Vec::new()
            }
        }
    })
    .await
    .map_err(|e| crate::ipc::IpcError::Other(format!("quota read task failed: {e}")))?;

    for quota in &quotas {
        let _ = app.emit("provider:quota-updated", quota);
    }
    Ok(quotas)
}
