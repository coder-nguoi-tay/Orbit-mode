use serde::{Deserialize, Serialize};

pub type PipelineId = i64;
pub type PipelineStepId = i64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Created,
    Preflight,
    Planning,
    PlanReady,
    Implementing,
    Reviewing,
    ChangesRequested,
    Testing,
    TestFailed,
    FinalReview,
    QualityGate,
    ReadyForHuman,
    Completed,
    Paused,
    Failed,
    Cancelled,
}

impl std::fmt::Display for PipelineStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        write!(f, "{s}")
    }
}

impl std::str::FromStr for PipelineStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineAgentRole {
    Planner,
    Developer,
    Reviewer,
    Tester,
    Auditor,
}

impl std::fmt::Display for PipelineAgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        write!(f, "{s}")
    }
}

impl std::str::FromStr for PipelineAgentRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStepStatus {
    Pending,
    Starting,
    Running,
    Waiting,
    Passed,
    Failed,
    NeedsChanges,
    Cancelled,
    Skipped,
}

impl std::fmt::Display for PipelineStepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        write!(f, "{s}")
    }
}

impl std::str::FromStr for PipelineStepStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineAgentConfig {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineLimits {
    pub max_plan_revisions: u32,
    pub max_review_loops: u32,
    pub max_test_fix_loops: u32,
    pub max_total_agent_runs: u32,
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self {
            max_plan_revisions: 2,
            max_review_loops: 3,
            max_test_fix_loops: 3,
            max_total_agent_runs: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConfig {
    pub planner: PipelineAgentConfig,
    pub developer: PipelineAgentConfig,
    pub reviewer: PipelineAgentConfig,
    pub tester: PipelineAgentConfig,
    pub limits: PipelineLimits,
    pub require_final_review: bool,
    pub require_tests: bool,
    pub quality_commands: Vec<String>,
}

impl PipelineConfig {
    pub fn default_preset(default_provider: &str, default_model: &str) -> Self {
        let mk = |p: &str, m: &str| PipelineAgentConfig {
            provider: p.to_string(),
            model: m.to_string(),
        };
        Self {
            planner: mk("claude-code", default_model),
            developer: mk(default_provider, default_model),
            reviewer: mk(default_provider, default_model),
            tester: mk(default_provider, default_model),
            limits: PipelineLimits::default(),
            require_final_review: true,
            require_tests: true,
            quality_commands: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStep {
    pub id: PipelineStepId,
    pub pipeline_id: PipelineId,
    pub role: PipelineAgentRole,
    pub status: PipelineStepStatus,
    pub provider_id: String,
    pub model: Option<String>,
    pub attempt: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
    pub session_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: PipelineId,
    pub project_id: Option<i64>,
    pub name: String,
    pub user_request: String,
    pub worktree_path: Option<String>,
    pub status: PipelineStatus,
    pub config: PipelineConfig,
    pub baseline_git_head: Option<String>,
    pub review_loops: u32,
    pub test_loops: u32,
    pub plan_revisions: u32,
    pub total_agent_runs: u32,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub steps: Vec<PipelineStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePipelineRequest {
    pub name: String,
    pub user_request: String,
    pub worktree_path: String,
    pub config: PipelineConfig,
}
