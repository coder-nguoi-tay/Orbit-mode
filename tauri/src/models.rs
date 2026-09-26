use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Working,
    Input,
    Idle,
    New,
}

impl AgentStatus {
    pub fn label(&self) -> &str {
        match self {
            AgentStatus::Working => "WORKING",
            AgentStatus::Input => "INPUT",
            AgentStatus::Idle => "IDLE",
            AgentStatus::New => "NEW",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub context_tokens: Option<u64>,
    #[serde(default)]
    pub context_limit: Option<u64>,
    #[serde(default)]
    pub estimated_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniLogEntry {
    pub tool: String,
    pub target: String,
    pub result: Option<String>,
    pub success: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInfo {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub utilization: f64,
    pub resets_at: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    pub provider: String,
    pub account_key: String,
    #[serde(default)]
    pub provider_account_id: Option<String>,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    pub updated_at: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountAuthType {
    #[serde(rename = "chat_gpt_authenticated")]
    ChatGPTAuthenticated,
    ApiKey,
    ManagedWorkspace,
}

impl AccountAuthType {
    /// Return the stable database value for an account authentication method.
    ///
    /// @return A non-sensitive authentication type identifier.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatGPTAuthenticated => "chat_gpt_authenticated",
            Self::ApiKey => "api_key",
            Self::ManagedWorkspace => "managed_workspace",
        }
    }

    /// Read a supported account authentication method from persisted metadata.
    ///
    /// @param value The database authentication type.
    /// @return The corresponding authentication method, if supported.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "chat_gpt_authenticated" => Some(Self::ChatGPTAuthenticated),
            "api_key" => Some(Self::ApiKey),
            "managed_workspace" => Some(Self::ManagedWorkspace),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Available,
    Busy,
    NearLimit,
    QuotaExceeded,
    AuthExpired,
    NeedsLogin,
    Unavailable,
    Unknown,
}

impl AccountStatus {
    /// Return the stable database value for an account availability state.
    ///
    /// @return The non-sensitive status identifier.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Busy => "busy",
            Self::NearLimit => "near_limit",
            Self::QuotaExceeded => "quota_exceeded",
            Self::AuthExpired => "auth_expired",
            Self::NeedsLogin => "needs_login",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }

    /// Read a persisted account state without assuming an unknown value is available.
    ///
    /// @param value The database status.
    /// @return The recognized status, or Unknown.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn from_db(value: &str) -> Self {
        match value {
            "available" => Self::Available,
            "busy" => Self::Busy,
            "near_limit" => Self::NearLimit,
            "quota_exceeded" => Self::QuotaExceeded,
            "auth_expired" => Self::AuthExpired,
            "needs_login" => Self::NeedsLogin,
            "unavailable" => Self::Unavailable,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    pub auth_type: AccountAuthType,
    pub status: AccountStatus,
    pub execution_scope: String,
    #[serde(skip_serializing)]
    pub profile_home: Option<String>,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAccountEvent {
    pub account_id: String,
    pub label: String,
    pub event: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageSnapshot {
    pub id: i64,
    pub session_id: i64,
    pub session_name: Option<String>,
    pub project_name: Option<String>,
    pub provider: String,
    pub provider_account_id: Option<String>,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub context_tokens: Option<u64>,
    pub context_limit: Option<u64>,
    pub context_percent: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUsageSummary {
    pub project_name: String,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub agent_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageSummary {
    pub model: String,
    pub provider: String,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageHistoryPoint {
    pub bucket_start: String,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageOverview {
    pub total_tokens_today: u64,
    pub total_cost_today: f64,
    pub active_agents_count: usize,
    pub total_sessions_count: usize,
    pub quotas: Vec<ProviderQuota>,
    pub project_summaries: Vec<ProjectUsageSummary>,
    pub model_summaries: Vec<ModelUsageSummary>,
    pub token_usage_history: Vec<TokenUsageHistoryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitInfo {
    pub status: String,
    pub rate_limit_type: String,
    pub utilization: f64,
    pub resets_at: Option<i64>,
    pub is_using_overage: bool,
    pub surpassed_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentState {
    pub session_id: String,
    pub project: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub status: AgentStatus,
    pub model: Option<String>,
    pub model_display: String,
    pub tokens: TokenUsage,
    pub context_percent: f64,
    pub subagents: Vec<SubagentInfo>,
    pub mini_log: Vec<MiniLogEntry>,
    pub pending_approval: Option<String>,
    pub pid: Option<i32>,
    pub started_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum JournalEntryType {
    User,
    Thinking,
    Assistant,
    ToolCall,
    ToolResult,
    System,
    Progress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub session_id: String,
    pub timestamp: String,
    pub entry_type: JournalEntryType,
    pub text: Option<String>,
    pub thinking: Option<String>,
    pub thinking_duration: Option<f64>,
    pub tool: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub output: Option<String>,
    pub exit_code: Option<i32>,
    pub lines_changed: Option<LinesChanged>,
    #[serde(default)]
    pub seq: u32,
    #[serde(default)]
    pub epoch: String,
}

impl Default for JournalEntry {
    /// Provides a zero-valued base for struct-update syntax (`..JournalEntry::default()`).
    /// Callers MUST override `entry_type`; `Assistant` here is a placeholder, not a semantic default.
    fn default() -> Self {
        JournalEntry {
            session_id: String::new(),
            timestamp: String::new(),
            entry_type: JournalEntryType::Assistant,
            text: None,
            thinking: None,
            thinking_duration: None,
            tool: None,
            tool_input: None,
            output: None,
            exit_code: None,
            lines_changed: None,
            seq: 0,
            epoch: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesChanged {
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub file_path: String,
    pub from_version: u32,
    pub to_version: u32,
    pub hunks: Vec<DiffHunk>,
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub cmd: String,
    pub desc: String,
    pub category: String,
}

/// The JSON structure format a provider uses to emit task lists in its stdout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskFormat {
    /// Claude Code: assistant→content[].tool_use(name=TodoWrite)
    ClaudeToolUse,
    /// OpenCode: tool_use→part.tool=todowrite
    OpenCodeToolUse,
    /// Codex: item.completed→item.type=todo_list
    CodexItemList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub active_form: Option<String>,
    pub status: String,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
}

/// Map raw model IDs to human-friendly display names.
pub fn model_display_name(model_id: &str) -> &str {
    match model_id {
        "default" => "Default",
        "opus" => "Opus",
        "sonnet" => "Sonnet",
        "haiku" => "Haiku",
        "claude-opus-4-7" => "Opus 4.7",
        "claude-opus-4-7[1m]" => "Opus 4.7 (1M)",
        "claude-opus-4-6" => "Opus 4.6",
        "claude-opus-4-6[1m]" => "Opus 4.6 (1M)",
        "claude-sonnet-4-6" => "Sonnet 4.6",
        "claude-haiku-4-5-20251001" => "Haiku 4.5",
        _ => model_id,
    }
}

/// Context window size for a given model ID.
pub fn context_window(model_id: &str) -> u64 {
    match model_id {
        "claude-opus-4-7[1m]" | "claude-opus-4-6[1m]" => 1_000_000,
        "claude-sonnet-4-6"
        | "claude-opus-4-7"
        | "claude-opus-4-6"
        | "claude-haiku-4-5-20251001" => 200_000,
        _ => 0,
    }
}

// Session ID type — SQLite AUTOINCREMENT rowid
pub type SessionId = i64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AttentionReason {
    Permission,
    Completed,
    Error,
    RateLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionState {
    pub requires_attention: bool,
    pub reason: Option<AttentionReason>,
    pub since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensure the ChatGPT auth type keeps the stable IPC/database spelling.
    ///
    /// @return No value; assertions cover both JSON directions.
    /// @throws Panic If serde changes the public account contract.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    #[test]
    fn should_serialize_chatgpt_auth_type_with_stable_name() {
        let encoded = serde_json::to_string(&AccountAuthType::ChatGPTAuthenticated).unwrap();
        assert_eq!(encoded, "\"chat_gpt_authenticated\"");
        let decoded: AccountAuthType = serde_json::from_str("\"chat_gpt_authenticated\"").unwrap();
        assert_eq!(decoded, AccountAuthType::ChatGPTAuthenticated);
    }

    #[test]
    fn should_construct_journal_entry_with_default() {
        let entry = JournalEntry {
            session_id: "123".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            entry_type: JournalEntryType::User,
            text: Some("hello".to_string()),
            ..JournalEntry::default()
        };
        assert_eq!(entry.thinking, None);
        assert_eq!(entry.tool, None);
        assert_eq!(entry.exit_code, None);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Initializing,
    Running,
    Waiting,
    Completed,
    Stopped,
    Error,
    NeedsAccountAction,
    ReadyToResume,
}

impl SessionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            SessionStatus::Initializing => "initializing",
            SessionStatus::Running => "running",
            SessionStatus::Waiting => "waiting",
            SessionStatus::Completed => "completed",
            SessionStatus::Stopped => "stopped",
            SessionStatus::Error => "error",
            SessionStatus::NeedsAccountAction => "needs_account_action",
            SessionStatus::ReadyToResume => "ready_to_resume",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl rusqlite::types::FromSql for SessionStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        Ok(match s.as_str() {
            "initializing" => SessionStatus::Initializing,
            "running" => SessionStatus::Running,
            "waiting" => SessionStatus::Waiting,
            "completed" => SessionStatus::Completed,
            "stopped" => SessionStatus::Stopped,
            "error" => SessionStatus::Error,
            "needs_account_action" => SessionStatus::NeedsAccountAction,
            "ready_to_resume" => SessionStatus::ReadyToResume,
            _ => {
                return Err(rusqlite::types::FromSqlError::Other(
                    format!("unknown SessionStatus: {s}").into(),
                ))
            }
        })
    }
}

impl rusqlite::types::ToSql for SessionStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(self.as_str()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: SessionId,
    pub project_id: Option<i64>,
    pub name: Option<String>,
    pub status: SessionStatus,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub permission_mode: String,
    pub model: Option<String>,
    pub provider: String,
    pub provider_account_id: Option<String>,
    pub pid: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
    // Runtime fields (not in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mini_log: Option<Vec<MiniLogEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention: Option<AttentionState>,
    #[serde(default = "default_true")]
    pub skip_permissions: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<SessionId>,
    #[serde(default)]
    pub depth: i32,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct CreateSessionRequest {
    pub project_path: String,
    pub prompt: String,
    pub model: Option<String>,
    pub permission_mode: String, // "ignore" | "approve"
    pub use_worktree: bool,
    pub session_name: Option<String>,
}
