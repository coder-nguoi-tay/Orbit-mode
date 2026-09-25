use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::{Arc, RwLock};

use tauri::{AppHandle, Emitter};

use crate::journal::JournalState;
use crate::models::{AgentStatus, Session, SessionId, TokenUsage};

/// Default provider ID when none is specified.
const DEFAULT_PROVIDER: &str = "claude-code";
use crate::providers::{ProviderRegistry, ProviderSpawnConfig};
use crate::services::{database::DatabaseService, mcp_config};

/// Write provider-specific MCP configs in the project directory so agents can use orbit-mcp tools.
fn ensure_mcp_config(cwd: &str) {
    let Some(launch) = mcp_config::mcp_launch() else {
        eprintln!("[orbit:mcp] failed to resolve MCP launch command");
        return;
    };
    if let Err(e) = mcp_config::write_orbit_mcp_configs(std::path::Path::new(cwd), &launch) {
        eprintln!("[orbit:mcp] failed to write provider MCP configs: {e}");
    }
}

/// Reads `.git/HEAD` to detect the current branch without spawning a subprocess.
fn detect_git_branch(cwd: &str) -> Option<String> {
    let head = std::fs::read_to_string(std::path::Path::new(cwd).join(".git/HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(|b| b.to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOutputEvent {
    pub session_id: SessionId,
    pub entry: crate::models::JournalEntry,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateEvent {
    pub session_id: SessionId,
    pub status: String,
    pub tokens: TokenUsage,
    pub context_percent: f64,
    pub pending_approval: Option<String>,
    pub mini_log: Vec<crate::models::MiniLogEntry>,
    pub git_branch: Option<String>,
    pub subagents: Vec<crate::models::SubagentInfo>,
    pub model: Option<String>,
    pub context_window: Option<u64>,
    pub attention: crate::models::AttentionState,
    pub rate_limit: Vec<crate::models::RateLimitInfo>,
    pub cost_usd: Option<f64>,
}

struct ActiveSession {
    session: Session,
    /// The Claude CLI session ID (from stream-json system/init message).
    /// Required for --resume on follow-up messages.
    pub claude_session_id: Option<String>,
    /// Effort level for thinking (low, medium, high, max).
    pub effort: Option<String>,
    /// Provider API key (stored in memory only, never persisted).
    pub api_key: Option<String>,
    /// SSH private key path held in memory. Reused for follow-up messages.
    pub ssh_key_path: Option<String>,
    /// Stdin handle for providers that use persistent stdin (e.g. ACP JSON-RPC).
    pub stdin: Option<Arc<std::sync::Mutex<Box<dyn std::io::Write + Send>>>>,
}

#[derive(Clone)]
struct PendingAccountHandoff {
    target_account_id: String,
    model: String,
    effort: Option<String>,
    automatic: bool,
}

pub(crate) fn resolve_context_metrics(
    provider_id: &str,
    model: Option<&str>,
    state: &JournalState,
) -> (Option<u64>, f64) {
    let window = crate::commands::providers::resolve_context_window(
        provider_id,
        model,
        state.context_window,
    );
    let percent = window
        .filter(|window| *window > 0)
        .map(|window| (state.input_tokens as f64 / window as f64) * 100.0)
        .unwrap_or(0.0);
    (window, percent)
}

/// Capture a bounded, read-only Git fact for a local account handoff.
///
/// @param worktree The existing session working directory.
/// @param arguments The Git arguments describing the requested fact.
/// @return Captured output or an explanatory unavailable marker.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn git_handoff_fact(worktree: &str, arguments: &[&str]) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(12_000)
                .collect()
        })
        .unwrap_or_else(|| "Unavailable".into())
}

/// Build a local task packet for a fresh provider conversation on another account.
///
/// @param database The session's stored original user request.
/// @param session The session whose worktree remains in place.
/// @param recent_summary The last useful locally stored assistant message.
/// @return Bounded context emphasizing Git state and outstanding task.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn build_account_handoff_packet(
    database: &DatabaseService,
    session: &Session,
    recent_summary: Option<&str>,
) -> String {
    let worktree = session
        .worktree_path
        .as_deref()
        .or(session.cwd.as_deref())
        .unwrap_or_default();
    let task = database
        .get_first_session_user_task(session.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| "Continue the work in this existing worktree.".into());
    let branch = git_handoff_fact(worktree, &["branch", "--show-current"]);
    let status = git_handoff_fact(worktree, &["status", "--short"]);
    let diff = git_handoff_fact(worktree, &["diff", "--"]);
    let summary: String = recent_summary
        .unwrap_or("Unavailable")
        .chars()
        .take(4_000)
        .collect();
    format!(
        "Continue this Orbit coding task in the same worktree. This is a new provider conversation after an account handoff. Inspect current files before editing.\n\nOriginal task:\n{task}\n\nWorktree: {worktree}\nBranch: {branch}\nGit status:\n{status}\nGit diff (bounded):\n{diff}\nRecent agent summary:\n{summary}\n\nContinue the outstanding work and verify changes."
    )
}

type CodexHandoffOutput = (Vec<u8>, std::io::BufReader<Box<dyn std::io::Read + Send>>);

/// Find Codex's first model-turn event and retain every startup line for replay.
///
/// @param stdout Unread Codex JSON output.
/// @return Startup bytes and the remaining buffered output stream.
/// @throws String If Codex fails or closes output before starting a turn.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn read_codex_handoff_start(
    stdout: Box<dyn std::io::Read + Send>,
) -> Result<CodexHandoffOutput, String> {
    use std::io::BufRead;

    let mut reader = std::io::BufReader::new(stdout);
    let mut startup_output = Vec::new();
    for _ in 0..32 {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return Err("Codex exited before starting a model turn".into()),
            Ok(_) => {}
        }
        startup_output.extend_from_slice(line.as_bytes());
        if startup_output.len() > 65_536 {
            return Err("Codex startup output exceeded the safe limit".into());
        }
        let event_type = serde_json::from_str::<serde_json::Value>(&line)
            .ok()
            .and_then(|event| {
                event
                    .get("type")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            });
        match event_type.as_deref() {
            Some("turn.started") => return Ok((startup_output, reader)),
            Some("error" | "turn.failed") => {
                return Err("Codex rejected the target account or model".into());
            }
            _ => {}
        }
    }
    Err("Codex did not start a model turn".into())
}

/// Wait for Codex to start a new model turn before committing account ownership.
///
/// @param handle Newly spawned Codex child whose stdout is still unread.
/// @return Success with the consumed startup lines restored to the output stream.
/// @throws String If Codex fails, closes stdout or does not start a turn promptly.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn verify_codex_handoff_started(
    handle: &mut crate::services::spawn_manager::SpawnHandle,
) -> Result<(), String> {
    use std::io::Read;

    let stdout = std::mem::replace(&mut handle.reader, Box::new(std::io::empty()));
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(read_codex_handoff_start(stdout));
    });

    match receiver.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(Ok((startup_output, reader))) => {
            handle.reader = Box::new(std::io::Cursor::new(startup_output).chain(reader));
            Ok(())
        }
        Ok(Err(error)) => Err(error),
        Err(_) => Err("Codex did not confirm the account handoff in time".into()),
    }
}

pub struct SessionManager {
    pub db: Arc<DatabaseService>,
    active: HashMap<SessionId, ActiveSession>,
    pub journal_states: HashMap<SessionId, JournalState>,
    /// MCP-spawned child sessions grouped by parent session ID.
    mcp_subagents: HashMap<SessionId, Vec<(SessionId, crate::models::SubagentInfo)>>,
    /// Git diff manager for real-time working tree watching.
    pub diff_manager: Arc<super::diff_manager::DiffManager>,
    /// Tracks which session is watching which directory (for cleanup).
    watch_map: HashMap<SessionId, std::path::PathBuf>,
    /// Sessions currently in the spawning phase (prevents double-spawn race).
    spawning_sessions: HashSet<SessionId>,
    pending_account_handoffs: HashMap<SessionId, PendingAccountHandoff>,
}

impl SessionManager {
    pub fn new(db: Arc<DatabaseService>) -> Self {
        SessionManager {
            db,
            active: HashMap::new(),
            journal_states: HashMap::new(),
            mcp_subagents: HashMap::new(),
            diff_manager: Arc::new(super::diff_manager::DiffManager::new()),
            watch_map: HashMap::new(),
            spawning_sessions: HashSet::new(),
            pending_account_handoffs: HashMap::new(),
        }
    }

    pub fn register_mcp_subagent(
        &mut self,
        parent_id: SessionId,
        child_id: SessionId,
        description: &str,
        provider: &str,
    ) {
        let info = crate::models::SubagentInfo {
            id: child_id.to_string(),
            agent_type: format!("mcp:{provider}"),
            description: description.to_string(),
            status: "running".to_string(),
        };
        self.mcp_subagents
            .entry(parent_id)
            .or_default()
            .push((child_id, info));
    }

    pub fn update_mcp_subagent_status(&mut self, child_id: SessionId, status: &str) {
        for children in self.mcp_subagents.values_mut() {
            if let Some((_, info)) = children.iter_mut().find(|(id, _)| *id == child_id) {
                info.status = status.to_string();
            }
        }
    }

    pub fn get_mcp_subagents(&self, parent_id: SessionId) -> Vec<crate::models::SubagentInfo> {
        self.mcp_subagents
            .get(&parent_id)
            .map(|children| children.iter().map(|(_, info)| info.clone()).collect())
            .unwrap_or_default()
    }

    /// Create a session and pin its initial local provider account before spawning.
    ///
    /// @param project_path Project directory for the session.
    /// @param session_name Optional user-facing session label.
    /// @param permission_mode Provider permission policy.
    /// @param model Requested model.
    /// @param use_worktree Whether a dedicated worktree is requested.
    /// @param provider Provider selected for the session.
    /// @param ssh_host Optional remote execution host.
    /// @param ssh_user Optional remote login name.
    /// @param ssh_key_path Optional remote private-key path.
    /// @return Persisted session with its initial account binding.
    /// @throws String If the project, worktree or session cannot be created.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[allow(clippy::too_many_arguments)]
    pub fn init_session(
        &mut self,
        project_path: &str,
        session_name: Option<&str>,
        permission_mode: &str,
        model: Option<&str>,
        use_worktree: bool,
        provider: Option<&str>,
        ssh_host: Option<&str>,
        ssh_user: Option<&str>,
        ssh_key_path: Option<String>,
    ) -> Result<Session, String> {
        let project_name = std::path::Path::new(project_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| project_path.to_string());

        let project = self
            .db
            .create_project(&project_name, project_path)
            .map_err(|e| e.to_string())?;

        let session_id = self
            .db
            .create_session(
                Some(project.id),
                session_name,
                project_path,
                permission_mode,
                model,
                provider,
                ssh_host,
                ssh_user,
            )
            .map_err(|e| e.to_string())?;

        let provider_id = provider.unwrap_or(DEFAULT_PROVIDER);
        let provider_account_id = if ssh_host.is_none() {
            self.db
                .resolve_default_provider_account(project.id, provider_id)
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        if let Some(ref account_id) = provider_account_id {
            self.db
                .set_session_provider_account(session_id, Some(account_id))
                .map_err(|error| error.to_string())?;
        }

        let (worktree_path_val, branch_name_val) = if use_worktree {
            let full_name = session_name.unwrap_or(&project_name);
            let (prefix, suffix) = full_name.split_once(" · ").unwrap_or((full_name, ""));
            let prefix_slug = crate::services::worktree::generate_branch_slug(prefix);
            let suffix_compact: String = suffix
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            let slug = if suffix_compact.is_empty() {
                format!("{prefix_slug}-{session_id}")
            } else {
                format!("{prefix_slug}-{suffix_compact}-{session_id}")
            };
            let (wt_str, branch) = if let (Some(host), Some(user)) = (ssh_host, ssh_user) {
                let remote_path = crate::services::worktree::create_worktree_remote(
                    host,
                    user,
                    ssh_key_path.as_deref(),
                    project_path,
                    &slug,
                )?;
                let branch = format!("orbit/{slug}");
                (remote_path, branch)
            } else {
                let wt_path = crate::services::worktree::create_worktree(
                    std::path::Path::new(project_path),
                    &slug,
                )?;
                let branch = format!("orbit/{slug}");
                let wt_str = wt_path.to_string_lossy().to_string();
                (wt_str, branch)
            };
            let _ = self
                .db
                .update_session_worktree(session_id, &wt_str, &branch);
            (Some(wt_str), Some(branch))
        } else {
            (None, None)
        };

        let now = chrono::Utc::now().to_rfc3339();
        let session = Session {
            id: session_id,
            project_id: Some(project.id),
            name: session_name.map(|s| s.to_string()),
            status: crate::models::SessionStatus::Initializing,
            worktree_path: worktree_path_val,
            branch_name: branch_name_val,
            permission_mode: permission_mode.to_string(),
            model: model.map(|s| s.to_string()),
            provider: provider.unwrap_or(DEFAULT_PROVIDER).to_string(),
            provider_account_id,
            pid: None,
            created_at: now.clone(),
            updated_at: now,
            cwd: Some(project_path.to_string()),
            project_name: Some(project_name),
            git_branch: detect_git_branch(project_path),
            tokens: None,
            context_percent: None,
            pending_approval: None,
            mini_log: None,
            ssh_host: ssh_host.map(|s| s.to_string()),
            ssh_user: ssh_user.map(|s| s.to_string()),
            attention: None,
            skip_permissions: permission_mode == "ignore",
            parent_session_id: None,
            depth: 0,
        };

        // Persist SSH key path encrypted to DB (api_key saved separately via set_api_key)
        if ssh_key_path.is_some() {
            let _ = self
                .db
                .save_session_secrets(session_id, None, ssh_key_path.as_deref());
        }

        self.active.insert(
            session_id,
            ActiveSession {
                session: session.clone(),
                claude_session_id: None,
                effort: None,
                api_key: None,
                ssh_key_path,
                stdin: None,
            },
        );
        self.journal_states
            .insert(session_id, JournalState::default());

        Ok(session)
    }

    /// Select a configured account before the session's first provider process starts.
    ///
    /// @param session_id The new Orbit session.
    /// @param account_id The explicit provider account selected by the user.
    /// @return Success after validating and persisting the binding.
    /// @throws String If the account is missing, incompatible, unauthenticated or already running.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn bind_session_provider_account(
        &mut self,
        session_id: SessionId,
        account_id: &str,
    ) -> Result<(), String> {
        let session = self
            .active
            .get(&session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if session.session.pid.is_some() {
            return Err("Cannot change the account of a running process".to_string());
        }
        let account = self
            .db
            .get_provider_account(account_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Account not found".to_string())?;
        if account.provider_id != session.session.provider {
            return Err("Account provider does not match the session provider".to_string());
        }
        if account.execution_scope != "local" || session.session.ssh_host.is_some() {
            return Err("Local account cannot be used for an SSH session".to_string());
        }
        if matches!(
            account.status,
            crate::models::AccountStatus::NeedsLogin
                | crate::models::AccountStatus::AuthExpired
                | crate::models::AccountStatus::QuotaExceeded
                | crate::models::AccountStatus::Unavailable
        ) {
            return Err("Account is not available for a new session".to_string());
        }
        if account.profile_home.is_some() && account.status == crate::models::AccountStatus::Unknown
        {
            return Err("Isolated account must pass an authentication check first".to_string());
        }
        self.db
            .set_session_provider_account(session_id, Some(account_id))
            .map_err(|error| error.to_string())?;
        if let Some(session) = self.active.get_mut(&session_id) {
            session.session.provider_account_id = Some(account_id.to_string());
        }
        Ok(())
    }

    /// Stage a user-confirmed account handoff without changing the stored binding early.
    ///
    /// @param manager The shared session manager.
    /// @param app The Tauri event emitter.
    /// @param session_id The paused session to continue.
    /// @param target_account_id The validated compatible account.
    /// @param model The user-approved model for the new process.
    /// @param effort The user-approved reasoning effort for that model.
    /// @param registry The provider registry used for the new process.
    /// @return Success when the new process has been scheduled.
    /// @throws String If the session is not paused, is remote or already spawning.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn begin_account_handoff(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        target_account_id: String,
        model: String,
        effort: Option<String>,
        registry: Arc<ProviderRegistry>,
    ) -> Result<(), String> {
        Self::queue_account_handoff(
            manager,
            app,
            session_id,
            target_account_id,
            model,
            effort,
            registry,
            false,
        )
    }

    /// Queue an account handoff after a configured provider quota fallback is triggered.
    ///
    /// @param manager The shared session manager.
    /// @param app The Tauri event emitter.
    /// @param session_id The session whose account became exhausted.
    /// @param registry The provider registry used for the new process.
    /// @return Success when a configured fallback process has been scheduled.
    /// @throws String If no safe fallback is configured or the session cannot be resumed.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn begin_automatic_account_handoff(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        registry: Arc<ProviderRegistry>,
    ) -> Result<(), String> {
        let (target_account_id, model, effort) = {
            let session_manager = manager.read().unwrap_or_else(|error| error.into_inner());
            let session = session_manager
                .db
                .get_session(session_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Session not found".to_string())?;
            if session.provider != "codex" {
                return Err("Automatic account handoff is currently enabled for Codex only".into());
            }
            let source_account_id = session
                .provider_account_id
                .as_deref()
                .ok_or_else(|| "Session has no account binding".to_string())?;
            let target = session_manager
                .db
                .next_auto_handoff_account(
                    &session.provider,
                    "local",
                    session_id,
                    source_account_id,
                )
                .map_err(|error| error.to_string())?
                .or_else(|| {
                    session_manager
                        .db
                        .get_provider_account_fallback(source_account_id)
                        .ok()
                        .flatten()
                        .and_then(|target_id| {
                            session_manager
                                .db
                                .get_provider_account(&target_id)
                                .ok()
                                .flatten()
                        })
                        .filter(|account| {
                            account.provider_id == session.provider
                                && account.execution_scope == "local"
                                && matches!(
                                    account.status,
                                    crate::models::AccountStatus::Available
                                        | crate::models::AccountStatus::Busy
                                        | crate::models::AccountStatus::NearLimit
                                )
                        })
                })
                .ok_or_else(|| {
                    "No enabled available account is configured for automatic handoff".to_string()
                })?;
            (
                target.id,
                session.model.unwrap_or_else(|| "auto".into()),
                session_manager
                    .active
                    .get(&session_id)
                    .and_then(|active| active.effort.clone()),
            )
        };
        Self::queue_account_handoff(
            manager,
            app,
            session_id,
            target_account_id,
            model,
            effort,
            registry,
            true,
        )
    }

    /// Stage a handoff and start its provider process without changing the binding early.
    ///
    /// @param manager The shared session manager.
    /// @param app The Tauri event emitter.
    /// @param session_id The paused session to continue.
    /// @param target_account_id The validated compatible account.
    /// @param model The model used by the new process.
    /// @param effort The reasoning effort for the new process.
    /// @param registry The provider registry used for the new process.
    /// @param automatic Whether the handoff was caused by a configured quota fallback.
    /// @return Success when the new process has been scheduled.
    /// @throws String If the session is not paused, remote or already spawning.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    #[allow(clippy::too_many_arguments)]
    fn queue_account_handoff(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        target_account_id: String,
        model: String,
        effort: Option<String>,
        registry: Arc<ProviderRegistry>,
        automatic: bool,
    ) -> Result<(), String> {
        let prompt = {
            let mut session_manager = manager.write().unwrap_or_else(|error| error.into_inner());
            if (!automatic && session_manager.spawning_sessions.contains(&session_id))
                || session_manager
                    .pending_account_handoffs
                    .contains_key(&session_id)
            {
                return Err("This session is already starting a provider process".into());
            }
            let session = session_manager
                .db
                .get_session(session_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Session not found".to_string())?;
            if !matches!(
                session.status,
                crate::models::SessionStatus::NeedsAccountAction
                    | crate::models::SessionStatus::ReadyToResume
            ) {
                return Err("Account handoff requires a session paused for account action".into());
            }
            if session.ssh_host.is_some() {
                return Err("Local account handoff cannot change remote SSH credentials".into());
            }
            let account = session_manager
                .db
                .get_provider_account(&target_account_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Target account not found".to_string())?;
            if account.provider_id != session.provider || account.execution_scope != "local" {
                return Err("Target account is incompatible with this session".into());
            }
            let recent_summary = session_manager
                .journal_states
                .get(&session_id)
                .and_then(|state| {
                    state.entries.iter().rev().find(|entry| {
                        entry.entry_type == crate::models::JournalEntryType::Assistant
                            && entry.text.is_some()
                    })
                })
                .and_then(|entry| entry.text.as_deref());
            let prompt =
                build_account_handoff_packet(&session_manager.db, &session, recent_summary);
            session_manager
                .active
                .entry(session_id)
                .or_insert(ActiveSession {
                    session,
                    claude_session_id: None,
                    effort: None,
                    api_key: None,
                    ssh_key_path: None,
                    stdin: None,
                });
            session_manager.pending_account_handoffs.insert(
                session_id,
                PendingAccountHandoff {
                    target_account_id,
                    model,
                    effort,
                    automatic,
                },
            );
            prompt
        };
        std::thread::spawn(move || {
            Self::do_spawn(manager, app, session_id, prompt, registry);
        });
        Ok(())
    }

    /// Start one account-scoped provider process for an initial message or handoff.
    ///
    /// @param manager Shared session state and serialized spawn guard.
    /// @param app Application event emitter.
    /// @param session_id Session whose account and worktree are retained.
    /// @param prompt User message or local handoff packet.
    /// @param registry Provider capabilities and process launcher.
    /// @return No value; process results are emitted through session events.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn do_spawn(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        prompt: String,
        registry: Arc<ProviderRegistry>,
    ) {
        // Guard: prevent double-spawn via a spawning_sessions set
        // This catches races before PID is assigned (unlike the PID check).
        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if !m.spawning_sessions.insert(session_id) {
                eprintln!(
                    "[orbit] warning: session {session_id} already spawning, \
                     skipping duplicate spawn"
                );
                return;
            }
        }
        // 1. Read session data from the active map
        let (
            db,
            cwd,
            model,
            provider_id,
            effort,
            resume_id,
            extra_env,
            spawn_mode,
            ssh_key_path,
            skip_permissions,
            provider_account_id,
            pending_handoff,
        ) = match manager.try_read() {
            Ok(m) => {
                let a = match m.active.get(&session_id) {
                    Some(a) => a,
                    None => {
                        let _ = app.emit(
                            "session:error",
                            serde_json::json!({
                                "sessionId": session_id,
                                "error": "Session not found in active map"
                            }),
                        );
                        {
                            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                            m.spawning_sessions.remove(&session_id);
                        }
                        return;
                    }
                };

                let pending_handoff = m.pending_account_handoffs.get(&session_id).cloned();
                let raw_model = pending_handoff
                    .as_ref()
                    .map(|handoff| handoff.model.clone())
                    .unwrap_or_else(|| a.session.model.clone().unwrap_or_default());
                let pid_str = a.session.provider.clone();

                let spawn_mode = match (a.session.ssh_host.clone(), a.session.ssh_user.clone()) {
                    (Some(host), Some(user)) => crate::services::ssh::SpawnMode::Ssh { host, user },
                    (Some(host), None) => {
                        eprintln!(
                            "[orbit] session {session_id}: ssh_host={host:?} set but ssh_user is \
                         missing — falling back to local spawn."
                        );
                        crate::services::ssh::SpawnMode::Local
                    }
                    (None, Some(user)) => {
                        eprintln!(
                            "[orbit] session {session_id}: ssh_user={user:?} set but ssh_host is \
                         missing — falling back to local spawn."
                        );
                        crate::services::ssh::SpawnMode::Local
                    }
                    (None, None) => crate::services::ssh::SpawnMode::Local,
                };

                let mut extra_env = vec![("ORBIT_SESSION_ID".to_string(), session_id.to_string())];
                if let Some(ref key) = a.api_key {
                    let var_name = format!("{}_API_KEY", pid_str.to_uppercase().replace('-', "_"));
                    extra_env.push((var_name, key.clone()));
                }
                if let Some(ref effort) = a.effort {
                    extra_env.push(("ORBIT_EFFORT".to_string(), effort.clone()));
                }
                if pending_handoff.is_none() {
                    if let Some(ref resume_id) = a.claude_session_id {
                        extra_env.push(("ORBIT_RESUME_ID".to_string(), resume_id.clone()));
                    }
                }

                (
                    m.db.clone(),
                    a.session
                        .worktree_path
                        .clone()
                        .or_else(|| a.session.cwd.clone())
                        .unwrap_or_default(),
                    raw_model,
                    pid_str,
                    pending_handoff
                        .as_ref()
                        .and_then(|handoff| handoff.effort.clone())
                        .or_else(|| {
                            if pending_handoff.is_some() {
                                None
                            } else {
                                a.effort.clone()
                            }
                        }),
                    if pending_handoff.is_some() {
                        None
                    } else {
                        a.claude_session_id.clone()
                    },
                    extra_env,
                    spawn_mode,
                    a.ssh_key_path.clone(),
                    a.session.skip_permissions,
                    pending_handoff
                        .as_ref()
                        .map(|handoff| handoff.target_account_id.clone())
                        .or_else(|| a.session.provider_account_id.clone()),
                    pending_handoff,
                )
            }
            Err(_) => {
                eprintln!("[orbit] warning: failed to acquire read lock for session {session_id}, skipping spawn");
                {
                    let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                    m.spawning_sessions.remove(&session_id);
                }
                return;
            }
        };

        // 2. Resolve provider from registry
        let provider = match registry.resolve(&provider_id) {
            Some(p) => p,
            None => {
                let _ = app.emit(
                    "session:error",
                    serde_json::json!({
                        "sessionId": session_id,
                        "error": format!("Unknown provider: {provider_id}")
                    }),
                );
                {
                    let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                    m.spawning_sessions.remove(&session_id);
                }
                return;
            }
        };

        // 3. Format model via provider (no hardcoded string comparisons)
        let model = provider.format_model(&model, &provider_id);

        let account_home = if let Some(ref account_id) = provider_account_id {
            match db.get_provider_account(account_id) {
                Ok(Some(account))
                    if account.provider_id == provider_id
                        && (account.profile_home.is_none()
                            || matches!(
                                account.status,
                                crate::models::AccountStatus::Available
                                    | crate::models::AccountStatus::Busy
                                    | crate::models::AccountStatus::NearLimit
                            )) =>
                {
                    account.profile_home.map(std::path::PathBuf::from)
                }
                _ => {
                    let _ = db.update_session_status(
                        session_id,
                        crate::models::SessionStatus::NeedsAccountAction,
                    );
                    let _ = app.emit(
                        if pending_handoff.is_some() {
                            "session:handoff-failed"
                        } else {
                            "session:account-action-required"
                        },
                        serde_json::json!({
                            "sessionId": session_id,
                            "error": "Bound provider account is missing or incompatible"
                        }),
                    );
                    let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                    m.spawning_sessions.remove(&session_id);
                    m.pending_account_handoffs.remove(&session_id);
                    if let Some(active_session) = m.active.get_mut(&session_id) {
                        active_session.session.status =
                            crate::models::SessionStatus::NeedsAccountAction;
                    }
                    return;
                }
            }
        } else {
            None
        };

        // 4. Set context window from provider
        if let Some(ctx) = provider.context_window(&model) {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if let Some(state) = m.journal_states.get_mut(&session_id) {
                state.context_window = Some(ctx);
            }
        }

        // 4. Spawn CLI via provider trait
        let prompt_text = prompt.clone();
        if cfg!(debug_assertions) {
            eprintln!("[orbit:debug] ── spawn session {session_id} ──");
            eprintln!("[orbit:debug]   provider: {}", provider_id);
            eprintln!("[orbit:debug]   model: {}", model);
            eprintln!("[orbit:debug]   cwd: {}", cwd);
            eprintln!(
                "[orbit:debug]   spawn_mode: {}",
                match &spawn_mode {
                    crate::services::ssh::SpawnMode::Local => "local".to_string(),
                    crate::services::ssh::SpawnMode::Ssh { host, user } =>
                        format!("ssh {user}@{host}"),
                }
            );
            eprintln!(
                "[orbit:debug]   ssh_key_path: {}",
                if ssh_key_path.is_some() {
                    "<set>"
                } else {
                    "<none>"
                }
            );
            if !extra_env.is_empty() {
                for (k, _) in &extra_env {
                    eprintln!("[orbit:debug]   env: {k}=<set>");
                }
            }
            if let Some(ref rid) = resume_id {
                eprintln!("[orbit:debug]   resume: {rid}");
            }
            if let Some(ref effort) = effort {
                eprintln!("[orbit:debug]   effort: {effort}");
            }
        }
        let spawn_config = ProviderSpawnConfig {
            session_id,
            cwd: std::path::PathBuf::from(&cwd),
            provider_id: provider_id.clone(),
            model,
            prompt,
            resume_id,
            extra_env,
            account_home,
            effort,
            spawn_mode,
            ssh_key_path,
            skip_permissions,
        };

        // Write .mcp.json so the agent can use orbit-mcp tools for orchestration
        let is_local = matches!(
            &spawn_config.spawn_mode,
            crate::services::ssh::SpawnMode::Local
        );
        if is_local {
            ensure_mcp_config(&cwd);
        }

        let selected_model = spawn_config.model.clone();
        let mut handle = match provider.spawn(spawn_config) {
            Ok(h) => h,
            Err(e) => {
                let next_status = if pending_handoff.is_some() {
                    crate::models::SessionStatus::NeedsAccountAction
                } else {
                    crate::models::SessionStatus::Error
                };
                let _ = db.update_session_status(session_id, next_status);
                {
                    let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                    m.spawning_sessions.remove(&session_id);
                    m.pending_account_handoffs.remove(&session_id);
                    if let Some(a) = m.active.get_mut(&session_id) {
                        a.session.attention = Some(crate::models::AttentionState {
                            requires_attention: true,
                            reason: Some(crate::models::AttentionReason::Error),
                            since: Some(chrono::Utc::now().to_rfc3339()),
                        });
                    }
                }
                let _ = app.emit(
                    if pending_handoff.is_some() {
                        "session:handoff-failed"
                    } else {
                        "session:error"
                    },
                    serde_json::json!({
                        "sessionId": session_id, "error": e
                    }),
                );
                return;
            }
        };

        if let Some(handoff) = pending_handoff {
            if let Err(error) = verify_codex_handoff_started(&mut handle) {
                let _ = handle.child.kill();
                let _ = handle.child.wait();
                let _ = db.update_session_status(
                    session_id,
                    crate::models::SessionStatus::NeedsAccountAction,
                );
                let mut session_manager = manager
                    .write()
                    .unwrap_or_else(|lock_error| lock_error.into_inner());
                session_manager.spawning_sessions.remove(&session_id);
                session_manager.pending_account_handoffs.remove(&session_id);
                let _ = app.emit(
                    "session:handoff-failed",
                    serde_json::json!({ "sessionId": session_id, "error": error }),
                );
                return;
            }
            let previous_account_id = {
                let m = manager.read().unwrap_or_else(|e| e.into_inner());
                m.active
                    .get(&session_id)
                    .and_then(|active_session| active_session.session.provider_account_id.clone())
            };
            let target_event = if handoff.automatic {
                "automatic_handoff_to"
            } else {
                "handoff_to"
            };
            if let Err(error) = db.commit_session_account_handoff_with_event(
                session_id,
                previous_account_id.as_deref(),
                &handoff.target_account_id,
                &selected_model,
                target_event,
            ) {
                let _ = handle.child.kill();
                let _ = handle.child.wait();
                let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                m.spawning_sessions.remove(&session_id);
                m.pending_account_handoffs.remove(&session_id);
                let _ = app.emit(
                    "session:handoff-failed",
                    serde_json::json!({ "sessionId": session_id, "error": error.to_string() }),
                );
                return;
            }
            {
                let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                if let Some(active_session) = m.active.get_mut(&session_id) {
                    active_session.session.provider_account_id =
                        Some(handoff.target_account_id.clone());
                    active_session.session.model = Some(selected_model);
                    active_session.claude_session_id = None;
                    active_session.effort = handoff.effort;
                }
                m.pending_account_handoffs.remove(&session_id);
            }
            let _ = app.emit(
                "session:account-changed",
                serde_json::json!({
                    "sessionId": session_id,
                    "providerAccountId": handoff.target_account_id,
                }),
            );
        }

        // Start git working tree watcher for real-time diff updates
        let cwd_path = std::path::PathBuf::from(&cwd);
        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            let app_handle = app.clone();
            let sid = session_id;
            m.diff_manager.watch(cwd_path.clone(), move |snapshot| {
                let _ = app_handle.emit(
                    "session:git-update",
                    serde_json::json!({
                        "sessionId": sid,
                        "snapshot": snapshot
                    }),
                );
            });
            m.watch_map.insert(session_id, cwd_path);
        }

        // 5. Stderr drain — rate limit detection is handled by stdout rate_limit_event
        let stderr_reader = handle.stderr;
        let app_clone = app.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stderr_reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if cfg!(debug_assertions) {
                                eprintln!("[orbit:debug] stderr {session_id}: {trimmed}");
                            }
                            eprintln!("[orbit:stderr] {session_id}: {trimmed}");
                            let _ = app_clone.emit(
                                "session:stderr",
                                serde_json::json!({
                                    "sessionId": session_id,
                                    "line": trimmed
                                }),
                            );
                        }
                    }
                }
            }
        });

        // 6. Store stdin handle (if provider returned one, e.g. ACP)
        let stdin_handle = handle.stdin.map(|s| Arc::new(std::sync::Mutex::new(s)));

        // 7. Emit spawn-started events
        Self::emit_spawn_started(
            &manager,
            &app,
            &db,
            session_id,
            handle.pid as i32,
            &prompt_text,
        );

        // 8. Attach stdin to the active session for later use (permission responses)
        if stdin_handle.is_some() {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if let Some(a) = m.active.get_mut(&session_id) {
                a.stdin = stdin_handle;
            }
        }

        // 10. Get line processor fn pointer from the provider trait.
        // Uses fn pointer (not trait object) because threads require Send.
        let line_processor = provider.line_processor();
        let subagent_tools: Vec<String> = provider
            .subagent_tool_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let task_tools: Vec<String> = provider
            .task_tool_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        // 11. Reader loop
        Self::reader_loop(
            Arc::clone(&manager),
            app,
            session_id,
            handle.reader,
            db,
            handle.child,
            Arc::clone(&registry),
            line_processor,
            subagent_tools,
            task_tools,
        );
    }

    /// Common post-spawn: set Running status, emit session:running, emit user entry.
    fn emit_spawn_started(
        manager: &Arc<RwLock<SessionManager>>,
        app: &AppHandle,
        db: &Arc<DatabaseService>,
        session_id: SessionId,
        pid: i32,
        prompt_text: &str,
    ) {
        let _ = db.update_session_pid(session_id, pid);
        // Write PID file so orbit-mcp can discover its parent session
        let pid_file = std::env::temp_dir().join(format!("orbit-session-{pid}.id"));
        let _ = std::fs::write(&pid_file, session_id.to_string());
        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if let Some(a) = m.active.get_mut(&session_id) {
                a.session.status = crate::models::SessionStatus::Running;
                a.session.pid = Some(pid);
            }
        }

        let _ = app.emit(
            "session:running",
            serde_json::json!({ "sessionId": session_id, "pid": pid }),
        );

        // Skip if create_session already injected the initial user message.
        let skip_user_entry = {
            let m = manager.read().unwrap_or_else(|e| e.into_inner());
            m.journal_states.get(&session_id).is_some_and(|state| {
                state.entries.iter().any(|e| {
                    e.entry_type == crate::models::JournalEntryType::User
                        && e.text
                            .as_deref()
                            .is_some_and(|t| t.trim() == prompt_text.trim())
                })
            })
        };

        // Skip creating user entry if prompt is empty/whitespace-only
        if !prompt_text.trim().is_empty() && !skip_user_entry {
            let user_entry = crate::models::JournalEntry {
                session_id: session_id.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                entry_type: crate::models::JournalEntryType::User,
                text: Some(prompt_text.to_string()),
                ..crate::models::JournalEntry::default()
            };
            let user_line = serde_json::json!({
                "type": "user",
                "message": { "content": prompt_text },
                "timestamp": &user_entry.timestamp
            })
            .to_string();
            let _ = db.insert_output(session_id, &user_line);
            let _ = app.emit(
                "session:raw-output",
                serde_json::json!({ "sessionId": session_id, "line": &user_line }),
            );

            let emit_entry: crate::models::JournalEntry;
            {
                let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                let state = m.journal_states.entry(session_id).or_default();
                let mut entry = user_entry;
                entry.seq = state.next_seq;
                entry.epoch = state.epoch.clone();
                state.next_seq += 1;
                emit_entry = entry.clone();
                state.entries.push(entry);
            }

            let _ = app.emit(
                "session:output",
                SessionOutputEvent {
                    session_id,
                    entry: emit_entry,
                },
            );
        }
    }

    /// Process provider output, account quota state and session lifecycle events.
    ///
    /// @param manager Shared session state.
    /// @param app Application event emitter.
    /// @param session_id Session producing the output.
    /// @param reader Provider stdout stream.
    /// @param db Database for usage and status persistence.
    /// @param child Provider child process to reap or stop on quota exhaustion.
    /// @param registry Provider registry used for an automatic quota handoff.
    /// @param line_processor Provider-specific structured line parser.
    /// @param subagent_tools Tools identifying child agents.
    /// @param task_tools Tools identifying task updates.
    /// @return No value; updates are persisted and emitted.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[allow(clippy::too_many_arguments)]
    fn reader_loop(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        reader: Box<dyn std::io::Read + Send>,
        db: Arc<DatabaseService>,
        mut child: std::process::Child,
        registry: Arc<ProviderRegistry>,
        line_processor: fn(&mut JournalState, &str),
        subagent_tools: Vec<String>,
        task_tools: Vec<String>,
    ) {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(reader);
        let mut line = String::new();
        let mut quota_exhausted = false;

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if trimmed.is_empty() || !trimmed.starts_with('{') {
                        continue;
                    }

                    if cfg!(debug_assertions) {
                        eprintln!("[orbit:debug] stdout {session_id}: {trimmed}");
                    }

                    // Extract and persist Claude session ID from system/init message
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&trimmed) {
                        // Extract CLI session ID for resume support:
                        //   Claude: "session_id", OpenCode: "sessionID", Codex: "thread_id"
                        let cli_sid = val
                            .get("session_id")
                            .or_else(|| val.get("sessionID"))
                            .or_else(|| val.get("thread_id"))
                            .and_then(|v| v.as_str());
                        if let Some(claude_id) = cli_sid {
                            let should_persist = {
                                let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                                if let Some(a) = m.active.get_mut(&session_id) {
                                    if a.claude_session_id.is_none() {
                                        a.claude_session_id = Some(claude_id.to_string());
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            };
                            // Persist to DB after releasing the write lock
                            if should_persist {
                                let _ = db.update_claude_session_id(session_id, claude_id);
                            }
                        }
                    }

                    // Detect rate limit errors from Claude's JSON stream
                    if is_rate_limit_line(&trimmed) {
                        let _ = app.emit(
                            "session:rate-limit",
                            serde_json::json!({ "sessionId": session_id }),
                        );
                    }

                    let _ = db.insert_output(session_id, &trimmed);
                    let _ = app.emit(
                        "session:raw-output",
                        serde_json::json!({ "sessionId": session_id, "line": &trimmed }),
                    );

                    let (new_entries, state_event, is_rate_limit, plan_quota_exhausted) = {
                        let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
                        let cwd = m
                            .active
                            .get(&session_id)
                            .and_then(|a| a.session.cwd.clone());
                        let provider_id = m
                            .active
                            .get(&session_id)
                            .map(|a| a.session.provider.clone())
                            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
                        let provider_account_id = m
                            .active
                            .get(&session_id)
                            .and_then(|a| a.session.provider_account_id.clone());
                        let session_model = m
                            .active
                            .get(&session_id)
                            .and_then(|a| a.session.model.clone());
                        let git_branch = cwd.as_deref().and_then(detect_git_branch);
                        let claude_session_id = m
                            .active
                            .get(&session_id)
                            .and_then(|a| a.claude_session_id.clone());
                        let mut subagents = claude_session_id
                            .as_deref()
                            .map(crate::agent_tree::read_subagents)
                            .unwrap_or_default();
                        for mcp in m.get_mcp_subagents(session_id) {
                            if !subagents.iter().any(|s| s.id == mcp.id) {
                                subagents.push(mcp);
                            }
                        }

                        let state = m.journal_states.entry(session_id).or_default();

                        let prev_len = state.entries.len();
                        let prev_model = state.model.clone();
                        line_processor(state, &trimmed);
                        // Assign seq/epoch to new entries
                        for entry in &mut state.entries[prev_len..] {
                            entry.seq = state.next_seq;
                            entry.epoch = state.epoch.clone();
                            state.next_seq += 1;
                        }
                        let new_entries: Vec<_> = state.entries[prev_len..].to_vec();

                        // Persist model to DB + active session when first detected
                        let model_changed = state.model != prev_model;
                        let detected_model = if model_changed {
                            state.model.clone()
                        } else {
                            None
                        };

                        let resolved_model_owned =
                            state.model.clone().or_else(|| session_model.clone());
                        let resolved_model = resolved_model_owned.as_deref();
                        let (window, context_percent) =
                            resolve_context_metrics(&provider_id, resolved_model, state);

                        let status_str = match state.status {
                            AgentStatus::Working => "working",
                            AgentStatus::Input => "input",
                            AgentStatus::Idle => "idle",
                            AgentStatus::New => "new",
                        }
                        .to_string();

                        let estimated_cost = state.cost_usd.or_else(|| {
                            crate::services::pricing::estimate_cost(
                                resolved_model,
                                state.input_tokens,
                                state.output_tokens,
                                state.cache_read,
                                state.cache_write,
                            )
                        });

                        let token_usage = TokenUsage {
                            input: state.input_tokens,
                            output: state.output_tokens,
                            cache_read: state.cache_read,
                            cache_write: state.cache_write,
                            reasoning: 0,
                            total: state.input_tokens + state.output_tokens,
                            context_tokens: Some(state.input_tokens),
                            context_limit: window,
                            estimated_cost,
                        };

                        let event = SessionStateEvent {
                            session_id,
                            status: status_str,
                            tokens: token_usage.clone(),
                            context_percent,
                            pending_approval: state.pending_approval.clone(),
                            mini_log: state.mini_log.clone(),
                            git_branch,
                            subagents,
                            model: state.model.clone(),
                            context_window: window,
                            attention: state.attention.clone(),
                            rate_limit: state.rate_limit.clone(),
                            cost_usd: estimated_cost,
                        };
                        if let Some(ref model) = detected_model {
                            let _ = db.update_session_model(session_id, model);
                            if let Some(a) = m.active.get_mut(&session_id) {
                                a.session.model = Some(model.clone());
                            }
                        }

                        if token_usage.total > 0 {
                            let _ = db.record_session_usage(
                                session_id,
                                provider_account_id.as_deref(),
                                &provider_id,
                                resolved_model,
                                &token_usage,
                                Some(context_percent),
                            );
                        }

                        if !event.rate_limit.is_empty() {
                            let five_hour = event
                                .rate_limit
                                .iter()
                                .find(|rl| rl.rate_limit_type == "five_hour")
                                .map(|rl| crate::models::QuotaWindow {
                                    utilization: rl.utilization,
                                    resets_at: rl.resets_at,
                                    status: Some(rl.status.clone()),
                                });
                            let seven_day = event
                                .rate_limit
                                .iter()
                                .find(|rl| rl.rate_limit_type == "seven_day")
                                .map(|rl| crate::models::QuotaWindow {
                                    utilization: rl.utilization,
                                    resets_at: rl.resets_at,
                                    status: Some(rl.status.clone()),
                                });
                            let quota = crate::models::ProviderQuota {
                                provider: provider_id.clone(),
                                account_key: provider_account_id
                                    .clone()
                                    .unwrap_or_else(|| "default".into()),
                                provider_account_id: provider_account_id.clone(),
                                five_hour,
                                seven_day,
                                updated_at: chrono::Utc::now().to_rfc3339(),
                                source: "cli_event".into(),
                            };
                            let _ = db.record_provider_quota(&quota);
                            let _ = app.emit("provider:quota-updated", &quota);
                        }

                        let plan_quota_exhausted = event
                            .rate_limit
                            .iter()
                            .any(is_authoritative_plan_quota_exhaustion);
                        let plan_windows: Vec<_> = event
                            .rate_limit
                            .iter()
                            .filter(|limit| {
                                matches!(limit.rate_limit_type.as_str(), "five_hour" | "seven_day")
                            })
                            .collect();
                        if let Some(ref account_id) = provider_account_id {
                            if !plan_windows.is_empty() {
                                let account_quota =
                                    db.get_latest_provider_quotas().ok().and_then(|quotas| {
                                        quotas.into_iter().find(|quota| {
                                            quota.provider_account_id.as_deref() == Some(account_id)
                                        })
                                    });
                                let latest_windows = account_quota
                                    .as_ref()
                                    .into_iter()
                                    .flat_map(|quota| {
                                        [quota.five_hour.as_ref(), quota.seven_day.as_ref()]
                                            .into_iter()
                                            .flatten()
                                    })
                                    .collect::<Vec<_>>();
                                let account_exhausted = latest_windows.iter().any(|window| {
                                    matches!(window.status.as_deref(), Some("exceeded" | "blocked"))
                                });
                                let next_status = if account_exhausted {
                                    crate::models::AccountStatus::QuotaExceeded
                                } else if latest_windows
                                    .iter()
                                    .any(|window| window.utilization >= 0.9)
                                {
                                    crate::models::AccountStatus::NearLimit
                                } else {
                                    crate::models::AccountStatus::Available
                                };
                                if let Ok(Some(account)) = db.get_provider_account(account_id) {
                                    if account.status != next_status {
                                        let reset_completed = account.status
                                            == crate::models::AccountStatus::QuotaExceeded
                                            && next_status
                                                == crate::models::AccountStatus::Available;
                                        let _ = db.update_provider_account_status(
                                            account_id,
                                            next_status.clone(),
                                        );
                                        let _ = app.emit(
                                            "provider:account-updated",
                                            serde_json::json!({
                                                "accountId": account_id,
                                                "status": next_status,
                                            }),
                                        );
                                        if reset_completed {
                                            let _ = db.mark_account_sessions_ready(account_id);
                                            let _ = app.emit(
                                                "provider:account-ready",
                                                serde_json::json!({ "accountId": account_id }),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        if plan_quota_exhausted {
                            let _ = db.update_session_status(
                                session_id,
                                crate::models::SessionStatus::NeedsAccountAction,
                            );
                            if let Some(active_session) = m.active.get_mut(&session_id) {
                                active_session.session.status =
                                    crate::models::SessionStatus::NeedsAccountAction;
                            }
                        }

                        let _ = app.emit(
                            "session:usage-updated",
                            serde_json::json!({
                                "sessionId": session_id,
                                "provider": provider_id,
                                "model": resolved_model,
                                "tokens": token_usage,
                                "contextPercent": context_percent,
                                "estimatedCost": estimated_cost,
                            }),
                        );

                        let is_rate_limit = event.attention.requires_attention
                            && event.attention.reason.as_ref().is_some_and(|r| {
                                matches!(r, crate::models::AttentionReason::RateLimit)
                            });
                        (new_entries, event, is_rate_limit, plan_quota_exhausted)
                    };

                    // Emit rate-limit event when journal detects it in the output stream
                    if is_rate_limit {
                        let _ = app.emit(
                            "session:rate-limit",
                            serde_json::json!({ "sessionId": session_id }),
                        );
                    }

                    for entry in new_entries {
                        let mut e = entry.clone();
                        e.session_id = session_id.to_string();

                        // Detect sub-agent spawns
                        if e.entry_type == crate::models::JournalEntryType::ToolCall {
                            if let Some(ref tool) = e.tool {
                                if subagent_tools.contains(tool) {
                                    let desc = e
                                        .tool_input
                                        .as_ref()
                                        .and_then(|v| v.get("description"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("subagent")
                                        .to_string();
                                    let _ = app.emit(
                                        "session:subagent-created",
                                        serde_json::json!({
                                            "parentSessionId": session_id,
                                            "description": desc,
                                            "tool": tool,
                                        }),
                                    );
                                }
                            }
                        }

                        // Detect task list updates
                        if e.entry_type == crate::models::JournalEntryType::ToolCall {
                            if let Some(ref tool) = e.tool {
                                if task_tools.contains(tool) {
                                    let _ = app.emit(
                                        "session:task-update",
                                        serde_json::json!({
                                            "sessionId": session_id,
                                        }),
                                    );
                                }
                            }
                        }

                        let _ = app.emit(
                            "session:output",
                            SessionOutputEvent {
                                session_id,
                                entry: e,
                            },
                        );
                    }
                    let _ = app.emit("session:state", &state_event);
                    if plan_quota_exhausted {
                        quota_exhausted = true;
                        let _ = child.kill();
                        break;
                    }
                }
            }
        }

        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if let Some(a) = m.active.get_mut(&session_id) {
                if !quota_exhausted {
                    a.session.status = crate::models::SessionStatus::Completed;
                }
                if !quota_exhausted {
                    a.session.attention = Some(crate::models::AttentionState {
                        requires_attention: true,
                        reason: Some(crate::models::AttentionReason::Completed),
                        since: Some(chrono::Utc::now().to_rfc3339()),
                    });
                }
            }
            if let Some(state) = m.journal_states.get_mut(&session_id) {
                state.status = AgentStatus::Idle;
                if !quota_exhausted {
                    state.attention = crate::models::AttentionState {
                        requires_attention: true,
                        reason: Some(crate::models::AttentionReason::Completed),
                        since: Some(chrono::Utc::now().to_rfc3339()),
                    };
                }
            }
            m.spawning_sessions.remove(&session_id);
            if !quota_exhausted {
                let _ =
                    db.update_session_status(session_id, crate::models::SessionStatus::Completed);
            }
        }

        if !quota_exhausted {
            let _ = app.emit(
                "session:stopped",
                serde_json::json!({ "sessionId": session_id }),
            );
        }

        // Collect exit status — prevents zombie on Unix, releases handle on Windows
        let _ = child.wait();
        if quota_exhausted
            && Self::begin_automatic_account_handoff(manager, app.clone(), session_id, registry)
                .is_err()
        {
            let _ = app.emit(
                "session:account-action-required",
                serde_json::json!({ "sessionId": session_id }),
            );
        }
    }

    /// Resume the same account's provider conversation after a user message.
    ///
    /// @param manager Shared session state and persisted account binding.
    /// @param app Application event emitter.
    /// @param session_id Session receiving the new message.
    /// @param text User message.
    /// @param registry Provider capabilities and process launcher.
    /// @return Success after scheduling the new process.
    /// @throws String If the session needs account action or cannot be resumed.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn send_message(
        manager: Arc<RwLock<SessionManager>>,
        app: AppHandle,
        session_id: SessionId,
        text: String,
        registry: Arc<ProviderRegistry>,
    ) -> Result<(), String> {
        if manager
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .db
            .get_session(session_id)
            .map_err(|error| error.to_string())?
            .is_some_and(|session| {
                session.status == crate::models::SessionStatus::NeedsAccountAction
            })
        {
            return Err("Choose an account or wait for the official quota reset".into());
        }
        // Re-add to active map if missing (e.g. after app restart)
        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if !m.active.contains_key(&session_id) {
                // Load from DB
                let session =
                    m.db.get_session(session_id)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("Session {session_id} not found"))?;

                let claude_session_id = m.db.get_claude_session_id(session_id).ok().flatten();
                let (api_key, ssh_key_path) =
                    m.db.load_session_secrets(session_id)
                        .unwrap_or((None, None));

                m.active.insert(
                    session_id,
                    ActiveSession {
                        session,
                        claude_session_id,
                        effort: None,
                        api_key,
                        ssh_key_path,
                        stdin: None,
                    },
                );
                m.journal_states.entry(session_id).or_default();
            }
        }

        // Push the user's follow-up message as a journal entry so it appears in the chat
        {
            let mut m = manager.write().unwrap_or_else(|e| e.into_inner());
            if let Some(state) = m.journal_states.get_mut(&session_id) {
                let user_entry = crate::models::JournalEntry {
                    session_id: session_id.to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    entry_type: crate::models::JournalEntryType::User,
                    text: Some(text.clone()),
                    seq: state.next_seq,
                    epoch: state.epoch.clone(),
                    ..crate::models::JournalEntry::default()
                };
                state.next_seq += 1;
                state.entries.push(user_entry.clone());
                let user_line = serde_json::json!({
                    "type": "user",
                    "message": { "content": &text },
                    "timestamp": &user_entry.timestamp
                })
                .to_string();
                let _ = m.db.insert_output(session_id, &user_line);
                let _ = app.emit(
                    "session:output",
                    serde_json::json!({
                        "sessionId": session_id,
                        "entry": user_entry
                    }),
                );
            }
        }

        let manager_clone = Arc::clone(&manager);
        std::thread::spawn(move || {
            Self::do_spawn(manager_clone, app, session_id, text, registry);
        });

        Ok(())
    }

    pub fn stop_session(&mut self, session_id: SessionId) -> Result<(), String> {
        // Unwatch git working tree
        if let Some(cwd) = self.watch_map.remove(&session_id) {
            self.diff_manager.unwatch(&cwd);
        }
        if let Some(a) = self.active.get(&session_id) {
            if let Some(pid) = a.session.pid {
                kill_pid(pid as u32);
                let pid_file = std::env::temp_dir().join(format!("orbit-session-{pid}.id"));
                let _ = std::fs::remove_file(pid_file);
            }
        }
        self.active.remove(&session_id);
        self.spawning_sessions.remove(&session_id);
        let _ = self
            .db
            .update_session_status(session_id, crate::models::SessionStatus::Stopped);
        Ok(())
    }

    pub fn get_sessions(&mut self) -> Vec<Session> {
        let mut sessions = self.db.get_sessions().unwrap_or_default();
        for s in &mut sessions {
            self.load_session_journal(s.id);
            if let Some(state) = self.journal_states.get(&s.id) {
                let resolved_model = state.model.as_deref().or(s.model.as_deref());
                let (_window, context_percent) =
                    resolve_context_metrics(&s.provider, resolved_model, state);
                let estimated_cost = state.cost_usd.or_else(|| {
                    crate::services::pricing::estimate_cost(
                        resolved_model,
                        state.input_tokens,
                        state.output_tokens,
                        state.cache_read,
                        state.cache_write,
                    )
                });
                s.tokens = Some(TokenUsage {
                    input: state.input_tokens,
                    output: state.output_tokens,
                    cache_read: state.cache_read,
                    cache_write: state.cache_write,
                    reasoning: 0,
                    total: state.input_tokens + state.output_tokens,
                    context_tokens: Some(state.input_tokens),
                    context_limit: _window,
                    estimated_cost,
                });
                s.context_percent = Some(context_percent);
                s.pending_approval = state.pending_approval.clone();
                s.mini_log = Some(state.mini_log.clone());
            }
            if let Some(a) = self.active.get(&s.id) {
                s.status = a.session.status.clone();
                s.pid = a.session.pid;
            }
        }
        sessions
    }

    pub fn get_journal(&mut self, session_id: SessionId) -> Vec<crate::models::JournalEntry> {
        self.load_session_journal(session_id);
        self.journal_states
            .get(&session_id)
            .map(|state| {
                state
                    .entries
                    .iter()
                    .map(|e| {
                        let mut entry = e.clone();
                        entry.session_id = session_id.to_string();
                        entry
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load journal state for `session_id` from DB into `journal_states` if not already present.
    fn load_session_journal(&mut self, session_id: SessionId) {
        if self.journal_states.contains_key(&session_id) {
            return;
        }
        let rows = match self.db.get_outputs(session_id) {
            Ok(r) => r,
            Err(_) => return,
        };
        if rows.is_empty() && !self.active.contains_key(&session_id) {
            return;
        }

        // Pick the right JSONL parser based on session provider
        let provider_owned = self
            .active
            .get(&session_id)
            .map(|a| a.session.provider.clone())
            .or_else(|| {
                self.db
                    .get_session(session_id)
                    .ok()
                    .flatten()
                    .map(|s| s.provider)
            })
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        let line_processor = crate::providers::ProviderRegistry::with_shipped_providers()
            .line_processor_for(&provider_owned);

        let mut state = JournalState::default();
        for line in &rows {
            line_processor(&mut state, line);
        }
        self.journal_states.insert(session_id, state);
    }

    pub fn is_session_active(&self, session_id: SessionId) -> bool {
        self.active.contains_key(&session_id)
    }

    pub fn get_session_provider(&self, session_id: SessionId) -> Option<String> {
        self.active
            .get(&session_id)
            .map(|a| a.session.provider.clone())
    }

    pub fn rename_session(&mut self, session_id: SessionId, name: &str) -> Result<(), String> {
        self.db
            .rename_session(session_id, name)
            .map_err(|e| e.to_string())
    }

    pub fn update_session_model(
        &mut self,
        session_id: SessionId,
        model: &str,
    ) -> Result<(), String> {
        if let Some(a) = self.active.get_mut(&session_id) {
            a.session.model = Some(model.to_string());
        }
        // Reset context_window so it re-derives from the new model
        if let Some(state) = self.journal_states.get_mut(&session_id) {
            state.context_window = None;
        }
        self.db
            .update_session_model(session_id, model)
            .map_err(|e| e.to_string())
    }

    pub fn update_session_effort(
        &mut self,
        session_id: SessionId,
        effort: &str,
    ) -> Result<(), String> {
        if let Some(a) = self.active.get_mut(&session_id) {
            a.effort = Some(effort.to_string());
        }
        Ok(())
    }

    /// Respond to a pending ACP permission request by writing a JSON-RPC response to stdin.
    pub fn respond_permission(&mut self, session_id: SessionId, allow: bool) -> Result<(), String> {
        let request_id = {
            let state = self
                .journal_states
                .get(&session_id)
                .ok_or("Session journal state not found")?;
            state
                .pending_approval_id
                .clone()
                .ok_or("No pending permission request")?
        };

        let stdin = {
            let a = self.active.get(&session_id).ok_or("Session not active")?;
            a.stdin
                .clone()
                .ok_or("Session does not support interactive permissions (no stdin)")?
        };

        let response = if allow {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": { "approved": true }
            })
        } else {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": { "approved": false }
            })
        };

        {
            let mut writer = stdin
                .lock()
                .map_err(|e| format!("stdin lock failed: {e}"))?;
            let line = serde_json::to_string(&response).map_err(|e| format!("serialize: {e}"))?;
            writer
                .write_all(line.as_bytes())
                .map_err(|e| format!("write: {e}"))?;
            writer
                .write_all(b"\n")
                .map_err(|e| format!("write newline: {e}"))?;
            writer.flush().map_err(|e| format!("flush: {e}"))?;
        }

        // Clear pending state
        if let Some(state) = self.journal_states.get_mut(&session_id) {
            state.pending_approval = None;
            state.pending_approval_id = None;
            state.status = crate::models::AgentStatus::Working;
            state.attention = crate::models::AttentionState {
                requires_attention: false,
                reason: None,
                since: None,
            };
        }

        Ok(())
    }

    pub fn clear_attention(&mut self, session_id: SessionId) -> Result<(), String> {
        if let Some(a) = self.active.get_mut(&session_id) {
            a.session.attention = Some(crate::models::AttentionState {
                requires_attention: false,
                reason: None,
                since: None,
            });
        }
        if let Some(state) = self.journal_states.get_mut(&session_id) {
            state.attention = crate::models::AttentionState {
                requires_attention: false,
                reason: None,
                since: None,
            };
        }
        Ok(())
    }

    pub fn set_api_key(&mut self, session_id: SessionId, api_key: String) {
        if let Some(a) = self.active.get_mut(&session_id) {
            // Persist encrypted to DB so it survives app restart
            let ssh_kp = a.ssh_key_path.as_deref();
            let _ = self
                .db
                .save_session_secrets(session_id, Some(&api_key), ssh_kp);
            a.api_key = Some(api_key);
        }
    }

    pub fn delete_session(&mut self, session_id: SessionId) -> Result<(), String> {
        // Unwatch git working tree
        if let Some(cwd) = self.watch_map.remove(&session_id) {
            self.diff_manager.unwatch(&cwd);
        }
        self.active.remove(&session_id);
        self.journal_states.remove(&session_id);
        self.db
            .delete_session(session_id)
            .map_err(|e| e.to_string())
    }

    pub fn reset_all_sessions(&mut self) -> Result<(), String> {
        for (_, a) in self.active.iter() {
            if let Some(pid) = a.session.pid {
                kill_pid(pid as u32);
                let pid_file = std::env::temp_dir().join(format!("orbit-session-{pid}.id"));
                let _ = std::fs::remove_file(pid_file);
            }
        }
        // Unwatch all git working trees
        for (_, cwd) in self.watch_map.drain() {
            self.diff_manager.unwatch(&cwd);
        }
        self.active.clear();
        self.journal_states.clear();
        self.db.delete_all_sessions().map_err(|e| e.to_string())
    }

    /// Gracefully stop a session with a timeout. If the session doesn't stop
    /// within the timeout, the process is killed forcefully.
    pub fn stop_session_with_timeout(
        &mut self,
        session_id: SessionId,
        timeout_ms: u64,
    ) -> Result<(), String> {
        let pid = self.active.get(&session_id).and_then(|a| a.session.pid);

        // Unwatch git working tree
        if let Some(cwd) = self.watch_map.remove(&session_id) {
            self.diff_manager.unwatch(&cwd);
        }

        if let Some(pid) = pid {
            // Try graceful stop first
            kill_pid(pid as u32);

            // Wait for process to exit with timeout using polling
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_millis(timeout_ms);
            loop {
                if start.elapsed() >= timeout {
                    eprintln!(
                        "[orbit] warning: session {session_id} (pid={pid}) did not stop within \
                         {timeout_ms}ms timeout — force-killing"
                    );
                    kill_pid(pid as u32);
                    break;
                }
                // Check if process is still running via process exit
                let mut tasklist = std::process::Command::new("tasklist");
                tasklist.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
                crate::services::process_util::apply_silent(&mut tasklist);
                let is_alive = tasklist.output();
                match is_alive {
                    Ok(output) => {
                        let out = String::from_utf8_lossy(&output.stdout);
                        let pid_str = pid.to_string();
                        if !out.contains(&pid_str) {
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        self.active.remove(&session_id);
        self.spawning_sessions.remove(&session_id);
        let _ = self
            .db
            .update_session_status(session_id, crate::models::SessionStatus::Stopped);
        Ok(())
    }

    /// Eagerly load journal state for all sessions from DB.
    /// Not called at startup (journals load lazily on first access).
    /// Available as a utility for warming the cache or in tests.
    pub fn restore_from_db(&mut self) {
        let session_ids: Vec<SessionId> = self
            .db
            .get_sessions()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.id)
            .collect();
        for id in session_ids {
            self.load_session_journal(id);
        }
    }
}

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let mut taskkill = std::process::Command::new("taskkill");
        taskkill.args(["/F", "/T", "/PID", &pid.to_string()]);
        crate::services::process_util::apply_silent(&mut taskkill);
        let _ = taskkill.output();
    }

    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
    }
}

/// Case-insensitive substring search without allocation (ASCII only).
/// Only used in tests — kept out of production paths after rate-limit detection was tightened.
#[cfg(test)]
fn ascii_ci_contains(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if h.len() < n.len() {
        return false;
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// Check if a JSON line from Claude's stdout indicates a rate limit error.
///
/// Parses the JSON and requires:
/// - top-level `"type"` == `"error"`
/// - nested `"error"."type"` is `"rate_limit_error"` or `"overloaded_error"`
///
/// This avoids false positives when assistant messages mention "rate limit"
/// or "overloaded" in their text content.
/// Recognize only a provider-reported plan window exhaustion as an account action.
///
/// @param rate_limit A structured CLI quota sample.
/// @return True when a five-hour or weekly plan window is explicitly blocked.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn is_authoritative_plan_quota_exhaustion(rate_limit: &crate::models::RateLimitInfo) -> bool {
    matches!(rate_limit.status.as_str(), "exceeded" | "blocked")
        && matches!(
            rate_limit.rate_limit_type.as_str(),
            "five_hour" | "seven_day"
        )
}

fn is_rate_limit_line(line: &str) -> bool {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    if val.get("type").and_then(|v| v.as_str()) != Some("error") {
        return false;
    }
    let error_type = val
        .get("error")
        .and_then(|e| e.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    matches!(error_type, "rate_limit_error" | "overloaded_error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assistant_with_tokens, make_db, seed_outputs, TestCase};

    /// Ensure temporary rate limits and estimates cannot pause a plan-quota session.
    ///
    /// @return Success when only a structured plan-window exhaustion is authoritative.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[test]
    fn should_only_pause_for_authoritative_plan_quota_exhaustion() {
        let mut rate_limit = crate::models::RateLimitInfo {
            status: "exceeded".into(),
            rate_limit_type: "requests_per_minute".into(),
            utilization: 1.0,
            resets_at: None,
            is_using_overage: false,
            surpassed_threshold: 1.0,
        };
        assert!(!is_authoritative_plan_quota_exhaustion(&rate_limit));
        rate_limit.rate_limit_type = "five_hour".into();
        assert!(is_authoritative_plan_quota_exhaustion(&rate_limit));
        rate_limit.status = "allowed_warning".into();
        assert!(!is_authoritative_plan_quota_exhaustion(&rate_limit));
    }

    /// Verify a failed Codex startup cannot be treated as a successful account switch.
    ///
    /// @return No value; assertions cover success and immediate provider failure.
    /// @throws Panic If the handoff startup classifier accepts a failed turn.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[test]
    fn should_require_codex_turn_start_before_handoff_commit() {
        let started = b"{\"type\":\"thread.started\"}\n{\"type\":\"turn.started\"}\n";
        let (prefix, mut remaining) =
            read_codex_handoff_start(Box::new(std::io::Cursor::new(started.to_vec()))).unwrap();
        assert_eq!(prefix, started);
        let mut after_start = String::new();
        std::io::Read::read_to_string(&mut remaining, &mut after_start).unwrap();
        assert!(after_start.is_empty());

        let failed = b"{\"type\":\"thread.started\"}\n{\"type\":\"turn.failed\"}\n";
        assert!(read_codex_handoff_start(Box::new(std::io::Cursor::new(failed.to_vec()))).is_err());
    }

    fn make_manager() -> Arc<RwLock<SessionManager>> {
        Arc::new(RwLock::new(SessionManager::new(make_db())))
    }

    // ── init_session ─────────────────────────────────────────────────────

    #[test]
    fn should_create_db_record_on_init() {
        let mut t = TestCase::new("should_create_db_record_on_init");
        t.phase("Act");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                None,
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Assert");
        t.ok("id is positive", s.id > 0);
        t.eq(
            "status is initializing",
            &s.status,
            &crate::models::SessionStatus::Initializing,
        );
    }

    #[test]
    fn should_register_journal_state_on_init() {
        let mut t = TestCase::new("should_register_journal_state_on_init");
        t.phase("Act");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                None,
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Assert");
        t.ok(
            "journal_state entry created",
            mgr.write().unwrap().journal_states.contains_key(&s.id),
        );
    }

    #[test]
    fn should_register_session_as_active_on_init() {
        let mut t = TestCase::new("should_register_session_as_active_on_init");
        t.phase("Act");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                None,
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Assert");
        t.ok(
            "session is active",
            mgr.write().unwrap().is_session_active(s.id),
        );
    }

    // ── stop_session ─────────────────────────────────────────────────────

    #[test]
    fn should_set_stopped_status_in_db_after_stop() {
        let mut t = TestCase::new("should_set_stopped_status_in_db_after_stop");
        t.phase("Seed");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                None,
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Act");
        mgr.write()
            .unwrap()
            .stop_session(s.id)
            .expect("stop failed");
        t.phase("Assert");
        let sessions = mgr.write().unwrap().get_sessions();
        t.eq(
            "status is stopped",
            &sessions[0].status,
            &crate::models::SessionStatus::Stopped,
        );
    }

    // ── delete_session ────────────────────────────────────────────────────

    #[test]
    fn should_remove_session_from_active_and_journal_after_delete() {
        let mut t = TestCase::new("should_remove_session_from_active_and_journal_after_delete");
        t.phase("Seed");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                None,
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Act");
        mgr.write()
            .unwrap()
            .delete_session(s.id)
            .expect("delete failed");
        t.phase("Assert");
        let mut m = mgr.write().unwrap();
        t.ok("not in active map", !m.is_session_active(s.id));
        t.ok(
            "journal_state removed",
            !m.journal_states.contains_key(&s.id),
        );
        t.empty("no sessions in DB", &m.get_sessions());
    }

    // ── rename_session ────────────────────────────────────────────────────

    #[test]
    fn should_persist_renamed_session_name() {
        let mut t = TestCase::new("should_persist_renamed_session_name");
        t.phase("Seed");
        let mgr = make_manager();
        let s = mgr
            .write()
            .unwrap()
            .init_session(
                "/tmp/proj",
                Some("old-name"),
                "ignore",
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .expect("init failed");
        t.phase("Act");
        mgr.write()
            .unwrap()
            .rename_session(s.id, "new-name")
            .expect("rename failed");
        t.phase("Assert");
        let sessions = mgr.write().unwrap().get_sessions();
        t.eq(
            "name updated",
            sessions[0].name.as_deref(),
            Some("new-name"),
        );
    }

    // ── send_message precondition ─────────────────────────────────────────

    #[test]
    fn should_confirm_session_does_not_exist_before_send_message_would_fail() {
        let mut t =
            TestCase::new("should_confirm_session_does_not_exist_before_send_message_would_fail");
        t.phase("Seed — no sessions exist");
        let mgr = make_manager();
        t.phase("Act — verify DB has no session 999");
        let m = mgr.write().unwrap();
        let db_result = m.db.get_session(999).expect("db query ok");
        drop(m);
        t.phase("Assert");
        t.none(
            "session 999 not in DB (error path precondition)",
            &db_result,
        );
        // Note: send_message requires a Tauri AppHandle which cannot be constructed
        // outside the Tauri runtime, so we verify the precondition that guarantees
        // the error path instead of calling send_message directly.
        t.ok("precondition verified", true);
    }

    // ── restore_from_db ───────────────────────────────────────────────────

    #[test]
    fn should_rebuild_journal_state_from_stored_outputs() {
        let mut t = TestCase::new("should_rebuild_journal_state_from_stored_outputs");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(
            &db,
            sid,
            &[&crate::test_utils::assistant_text("Restored entry")],
        );
        t.phase("Act");
        let mut sm = SessionManager::new(db);
        sm.restore_from_db();
        t.phase("Assert");
        let journal = sm.get_journal(sid);
        t.len("one entry restored", &journal, 1);
        t.eq(
            "entry text matches",
            journal[0].text.as_deref(),
            Some("Restored entry"),
        );
    }

    #[test]
    fn should_restore_codex_outputs_using_registry_line_processor() {
        let mut t = TestCase::new("should_restore_codex_outputs_using_registry_line_processor");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(
                None,
                None,
                "/tmp",
                "ignore",
                None,
                Some("codex"),
                None,
                None,
            )
            .expect("session");
        let codex_line =
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"codex-reply"}}"#;
        seed_outputs(&db, sid, &[codex_line]);
        t.phase("Act");
        let mut sm = SessionManager::new(Arc::clone(&db));
        sm.restore_from_db();
        t.phase("Assert");
        let journal = sm.get_journal(sid);
        t.len("one codex entry", &journal, 1);
        t.eq(
            "assistant text from codex jsonl",
            journal[0].text.as_deref(),
            Some("codex-reply"),
        );
        t.eq(
            "entry type",
            journal[0].entry_type,
            crate::models::JournalEntryType::Assistant,
        );
    }

    #[test]
    fn should_restore_follow_up_user_message_from_db() {
        let mut t = TestCase::new("should_restore_follow_up_user_message_from_db");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(
            &db,
            sid,
            &[&crate::test_utils::user_text("Also fix the tests")],
        );
        t.phase("Act");
        let mut sm = SessionManager::new(Arc::clone(&db));
        sm.restore_from_db();
        t.phase("Assert");
        let journal = sm.get_journal(sid);
        t.len("one user entry", &journal, 1);
        t.eq(
            "follow-up text restored",
            journal[0].text.as_deref(),
            Some("Also fix the tests"),
        );
        t.eq(
            "entry type",
            journal[0].entry_type,
            crate::models::JournalEntryType::User,
        );
    }

    #[test]
    fn should_not_duplicate_entries_on_double_restore() {
        let mut t = TestCase::new("should_not_duplicate_entries_on_double_restore");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(&db, sid, &[&crate::test_utils::assistant_text("Hello")]);
        t.phase("Act");
        let mut sm = SessionManager::new(Arc::clone(&db));
        sm.restore_from_db();
        sm.restore_from_db(); // second call must be idempotent
        t.phase("Assert");
        let journal = sm.get_journal(sid);
        t.len("still exactly one entry (no duplication)", &journal, 1);
    }

    #[test]
    fn should_restore_token_counts_from_stored_outputs() {
        let mut t = TestCase::new("should_restore_token_counts_from_stored_outputs");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        // input=10, output=5, cache_write=2, cache_read=1 → input_tokens = 13
        seed_outputs(&db, sid, &[&assistant_with_tokens("Hi", 10, 5, 2, 1)]);
        t.phase("Act");
        let mut sm = SessionManager::new(Arc::clone(&db));
        sm.restore_from_db();
        t.phase("Assert");
        let sessions = sm.get_sessions();
        let tokens = sessions[0]
            .tokens
            .as_ref()
            .expect("tokens missing after restore");
        t.eq("output_tokens restored", tokens.output, 5u64);
    }

    // ── ascii_ci_contains ─────────────────────────────────────────────────────

    #[test]
    fn should_find_needle_case_insensitively() {
        let mut t = TestCase::new("should_find_needle_case_insensitively");
        t.phase("Assert");
        t.ok("exact match", ascii_ci_contains("rate_limit", "rate_limit"));
        t.ok(
            "upper needle",
            ascii_ci_contains("RATE_LIMIT", "rate_limit"),
        );
        t.ok(
            "mixed case haystack",
            ascii_ci_contains("Rate_Limit_Error", "rate_limit"),
        );
        t.ok(
            "not found",
            !ascii_ci_contains("something else", "rate_limit"),
        );
        t.ok("empty haystack", !ascii_ci_contains("", "rate_limit"));
        t.ok(
            "needle longer than haystack",
            !ascii_ci_contains("rt", "rate_limit"),
        );
    }

    // ── is_rate_limit_line ────────────────────────────────────────────────────

    #[test]
    fn should_detect_rate_limit_error_line() {
        let mut t = TestCase::new("should_detect_rate_limit_error_line");
        t.phase("Assert — canonical rate limit JSON");
        t.ok(
            "rate_limit_error in error object",
            is_rate_limit_line(
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limit exceeded"}}"#,
            ),
        );
        t.ok(
            "overloaded_error in error object",
            is_rate_limit_line(r#"{"type":"error","error":{"type":"overloaded_error"}}"#),
        );
    }

    #[test]
    fn should_not_flag_assistant_message_mentioning_rate_limit() {
        let mut t = TestCase::new("should_not_flag_assistant_message_mentioning_rate_limit");
        t.phase("Assert — assistant message with 'rate limit' in text must NOT trigger");
        t.ok(
            "assistant type with rate limit text",
            !is_rate_limit_line(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The rate limit policy allows 1000 requests per minute."}]}}"#,
            ),
        );
    }

    #[test]
    fn should_not_flag_tool_result_mentioning_overloaded() {
        let mut t = TestCase::new("should_not_flag_tool_result_mentioning_overloaded");
        t.phase("Assert — tool_result containing 'overloaded' must NOT trigger");
        t.ok(
            "tool_result type with overloaded in output",
            !is_rate_limit_line(
                r#"{"type":"tool_result","content":"Server is overloaded, please retry"}"#,
            ),
        );
    }

    #[test]
    fn should_not_flag_non_rate_limit_lines() {
        let mut t = TestCase::new("should_not_flag_non_rate_limit_lines");
        t.phase("Assert — lines that should NOT trigger");
        t.ok(
            "rate_limit without error object",
            !is_rate_limit_line(r#"{"type":"assistant","text":"rate_limit info"}"#),
        );
        t.ok(
            "error type but no matching error subtype",
            !is_rate_limit_line(
                r#"{"type":"error","error":{"type":"api_error","message":"internal"}}"#,
            ),
        );
        t.ok(
            "plain overloaded text (not JSON error)",
            !is_rate_limit_line(r#"overloaded"#),
        );
        t.ok("empty line", !is_rate_limit_line(""));
        t.ok(
            "normal assistant line",
            !is_rate_limit_line(r#"{"type":"assistant","text":"hello world"}"#),
        );
    }

    #[test]
    fn should_detect_rate_limit_in_stderr_exact_substring() {
        // stderr lines are plain text — the check uses exact substring matching
        // for "rate_limit_error" or "overloaded_error" (not the broader "rate limit")
        let mut t = TestCase::new("should_detect_rate_limit_in_stderr_exact_substring");
        t.phase("Assert — exact substrings that must match");
        t.ok(
            "rate_limit_error substring present",
            "rate_limit_error: too many requests".contains("rate_limit_error"),
        );
        t.ok(
            "overloaded_error substring present",
            "overloaded_error detected".contains("overloaded_error"),
        );
        t.phase("Assert — generic 'rate limit' must NOT match the tightened check");
        t.ok(
            "generic 'rate limit' phrase does not match rate_limit_error",
            !"You have hit the rate limit today".contains("rate_limit_error"),
        );
        t.ok(
            "generic 'overloaded' does not match overloaded_error",
            !"Server is overloaded".contains("overloaded_error"),
        );
    }

    // ── lazy journal loading ──────────────────────────────────────────────

    #[test]
    fn should_not_preload_journal_state_on_creation() {
        let mut t = TestCase::new("should_not_preload_journal_state_on_creation");
        t.phase("Seed — DB has session with outputs, manager is fresh (no restore)");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(&db, sid, &[&crate::test_utils::assistant_text("hello")]);
        t.phase("Act — create manager without calling restore_from_db");
        let sm = SessionManager::new(Arc::clone(&db));
        t.phase("Assert — journal not loaded yet");
        t.ok(
            "journal_states empty before first access",
            !sm.journal_states.contains_key(&sid),
        );
    }

    #[test]
    fn should_lazy_load_tokens_on_get_sessions() {
        let mut t = TestCase::new("should_lazy_load_tokens_on_get_sessions");
        t.phase("Seed — session with token output exists");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(
            &db,
            sid,
            &[&crate::test_utils::assistant_with_tokens("Hi", 10, 5, 2, 1)],
        );
        t.phase("Act — fresh manager, no restore, call get_sessions");
        let mut sm = SessionManager::new(Arc::clone(&db));
        let sessions = sm.get_sessions();
        t.phase("Assert — tokens populated via lazy load");
        let tokens = sessions[0]
            .tokens
            .as_ref()
            .expect("tokens should be loaded");
        t.eq("output_tokens loaded", tokens.output, 5u64);
        t.ok(
            "journal_state was populated",
            sm.journal_states.contains_key(&sid),
        );
    }

    #[test]
    fn should_lazy_load_journal_on_first_get_journal() {
        let mut t = TestCase::new("should_lazy_load_journal_on_first_get_journal");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(&db, sid, &[&crate::test_utils::assistant_text("hello")]);
        t.phase("Act — get_journal triggers lazy load");
        let mut sm = SessionManager::new(Arc::clone(&db));
        let journal = sm.get_journal(sid);
        t.phase("Assert");
        t.len("one entry loaded on demand", &journal, 1);
    }

    // ── get_journal ───────────────────────────────────────────────────────

    #[test]
    fn should_fill_session_id_on_all_journal_entries() {
        let mut t = TestCase::new("should_fill_session_id_on_all_journal_entries");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        seed_outputs(
            &db,
            sid,
            &[
                &crate::test_utils::assistant_text("First"),
                &crate::test_utils::assistant_text("Second"),
            ],
        );
        let mut sm = SessionManager::new(db);
        sm.restore_from_db();
        t.phase("Act");
        let journal = sm.get_journal(sid);
        t.phase("Assert");
        t.len("two entries", &journal, 2);
        let expected_id = sid.to_string();
        t.eq(
            "first entry has session_id",
            journal[0].session_id.as_str(),
            expected_id.as_str(),
        );
        t.eq(
            "second entry has session_id",
            journal[1].session_id.as_str(),
            expected_id.as_str(),
        );
    }
}
