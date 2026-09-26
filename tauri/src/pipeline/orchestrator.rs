use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};

use crate::models::SessionStatus;
use crate::providers::ProviderRegistry;
use crate::services::database::DatabaseService;
use crate::services::session_manager::SessionManager;

use super::models::{
    CreatePipelineRequest, Pipeline, PipelineAgentRole, PipelineId, PipelineStatus, PipelineStepId,
    PipelineStepStatus,
};
use super::state_machine::{can_transition, is_terminal};

pub struct PipelineOrchestrator {
    pub db: Arc<DatabaseService>,
    pub session_manager: Arc<RwLock<SessionManager>>,
    pub registry: Arc<ProviderRegistry>,
}

impl PipelineOrchestrator {
    pub fn new(
        db: Arc<DatabaseService>,
        session_manager: Arc<RwLock<SessionManager>>,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self {
            db,
            session_manager,
            registry,
        }
    }

    pub fn create(&self, req: CreatePipelineRequest) -> Result<Pipeline, String> {
        let id = self
            .db
            .create_pipeline(
                &req.name,
                &req.user_request,
                Some(&req.worktree_path),
                &req.config,
            )
            .map_err(|e| e.to_string())?;

        let roles = [
            (PipelineAgentRole::Planner, &req.config.planner),
            (PipelineAgentRole::Developer, &req.config.developer),
            (PipelineAgentRole::Reviewer, &req.config.reviewer),
            (PipelineAgentRole::Tester, &req.config.tester),
        ];
        for (role, agent_cfg) in &roles {
            self.db
                .create_pipeline_step(id, role, &agent_cfg.provider, Some(&agent_cfg.model))
                .map_err(|e| e.to_string())?;
        }
        if req.config.require_final_review {
            self.db
                .create_pipeline_step(
                    id,
                    &PipelineAgentRole::Reviewer,
                    &req.config.reviewer.provider,
                    Some(&req.config.reviewer.model),
                )
                .map_err(|e| e.to_string())?;
        }

        self.db
            .append_pipeline_event(id, None, "pipeline_created", None)
            .ok();

        self.db
            .get_pipeline(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "pipeline not found after create".to_string())
    }

    pub fn transition(
        &self,
        id: PipelineId,
        to: PipelineStatus,
        app: &AppHandle,
    ) -> Result<Pipeline, String> {
        let pipeline = self
            .db
            .get_pipeline(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("pipeline {id} not found"))?;

        if is_terminal(&pipeline.status) {
            return Err(format!("pipeline {} is already terminal", pipeline.status));
        }
        if !can_transition(&pipeline.status, &to) {
            return Err(format!("invalid transition {} → {}", pipeline.status, to));
        }

        self.db
            .update_pipeline_status(id, &to)
            .map_err(|e| e.to_string())?;
        self.db
            .append_pipeline_event(
                id,
                None,
                &format!("status_changed_{to}"),
                Some(&serde_json::json!({ "from": pipeline.status.to_string(), "to": to.to_string() })),
            )
            .ok();

        let updated = self
            .db
            .get_pipeline(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "pipeline disappeared".to_string())?;

        let _ = app.emit(
            "pipeline:state",
            &serde_json::json!({
                "pipelineId": updated.id,
                "status": updated.status,
                "steps": updated.steps,
            }),
        );

        Ok(updated)
    }

    pub fn cancel(&self, id: PipelineId, app: &AppHandle) -> Result<Pipeline, String> {
        self.transition(id, PipelineStatus::Cancelled, app)
    }

    /// Launch the pipeline driver in a background thread.
    pub fn start(&self, pipeline_id: PipelineId, app: AppHandle) -> Result<(), String> {
        let db = Arc::clone(&self.db);
        let sm = Arc::clone(&self.session_manager);
        let reg = Arc::clone(&self.registry);
        std::thread::spawn(move || run_pipeline(pipeline_id, db, sm, reg, app));
        Ok(())
    }
}

// ── Pipeline driver (runs in a background thread) ───────────────────────────

fn run_pipeline(
    pipeline_id: PipelineId,
    db: Arc<DatabaseService>,
    sm: Arc<RwLock<SessionManager>>,
    reg: Arc<ProviderRegistry>,
    app: AppHandle,
) {
    let pipeline = match db.get_pipeline(pipeline_id) {
        Ok(Some(p)) => p,
        _ => return,
    };

    // Created → Preflight → Planning
    if advance(&db, &app, pipeline_id, PipelineStatus::Preflight).is_err()
        || advance(&db, &app, pipeline_id, PipelineStatus::Planning).is_err()
    {
        return;
    }

    // ── Planner step ─────────────────────────────────────────────────────────
    let planner_step = match pipeline
        .steps
        .iter()
        .find(|s| s.role == PipelineAgentRole::Planner)
    {
        Some(s) => s.clone(),
        None => {
            fail_pipeline(&db, &app, pipeline_id, "No planner step found");
            return;
        }
    };

    let plan_text = match run_step(
        &db,
        &sm,
        &reg,
        &app,
        pipeline_id,
        planner_step.id,
        &pipeline.config.planner.provider,
        &pipeline.config.planner.model,
        pipeline.worktree_path.as_deref().unwrap_or("."),
        &format!("pipeline-{pipeline_id}-planner"),
        build_planner_prompt(&pipeline),
    ) {
        Ok(out) => out,
        Err(e) => {
            fail_pipeline(&db, &app, pipeline_id, &e);
            return;
        }
    };

    // PlanReady: save the plan as an artifact
    let plan_json = parse_pipeline_result(&plan_text);
    db.save_pipeline_artifact(
        pipeline_id,
        Some(planner_step.id),
        "plan",
        &serde_json::json!({ "raw": plan_text, "structured": plan_json }),
    )
    .ok();
    if advance(&db, &app, pipeline_id, PipelineStatus::PlanReady).is_err() {
        return;
    }

    // ── Developer step ───────────────────────────────────────────────────────
    let pipeline = match db.get_pipeline(pipeline_id) {
        Ok(Some(p)) => p,
        _ => return,
    };
    let dev_step = match pipeline
        .steps
        .iter()
        .find(|s| s.role == PipelineAgentRole::Developer)
    {
        Some(s) => s.clone(),
        None => {
            fail_pipeline(&db, &app, pipeline_id, "No developer step found");
            return;
        }
    };

    if advance(&db, &app, pipeline_id, PipelineStatus::Implementing).is_err() {
        return;
    }

    let dev_prompt = build_developer_prompt(&pipeline, &plan_text);
    match run_step(
        &db,
        &sm,
        &reg,
        &app,
        pipeline_id,
        dev_step.id,
        &pipeline.config.developer.provider,
        &pipeline.config.developer.model,
        pipeline.worktree_path.as_deref().unwrap_or("."),
        &format!("pipeline-{pipeline_id}-developer"),
        dev_prompt,
    ) {
        Ok(_) => {}
        Err(e) => {
            fail_pipeline(&db, &app, pipeline_id, &e);
            return;
        }
    }

    // Phase 2: Developer done → ReadyForHuman (skip review/test/QG loops)
    // ponytail: skips reviewer/tester/QG loops — add those in Phase 3/4/5
    let _ = advance(&db, &app, pipeline_id, PipelineStatus::ReadyForHuman);
    let _ = app.emit(
        "pipeline:completed",
        &serde_json::json!({ "pipelineId": pipeline_id }),
    );
}

/// Run one pipeline step: spawn session, wait for completion, return full output.
#[allow(clippy::too_many_arguments)]
fn run_step(
    db: &Arc<DatabaseService>,
    sm: &Arc<RwLock<SessionManager>>,
    reg: &Arc<ProviderRegistry>,
    app: &AppHandle,
    pipeline_id: PipelineId,
    step_id: PipelineStepId,
    provider: &str,
    model: &str,
    cwd: &str,
    session_name: &str,
    prompt: String,
) -> Result<String, String> {
    let model_opt = if model.is_empty() || model == "auto" {
        None
    } else {
        Some(model)
    };

    let session = {
        let mut m = sm.write().unwrap_or_else(|e| e.into_inner());
        m.init_session(
            cwd,
            Some(session_name),
            "ignore", // --dangerously-skip-permissions
            model_opt,
            false,
            Some(provider),
            None,
            None,
            None,
        )
        .map_err(|e| e.to_string())?
    };

    // Emit session:created so the frontend tracks this session in the sidebar
    let _ = app.emit("session:created", &session);

    db.link_step_session(step_id, session.id, 1).ok();
    db.update_pipeline_step_status(step_id, &PipelineStepStatus::Running, None)
        .ok();
    emit_pipeline_state(db, app, pipeline_id);

    let sm_clone = Arc::clone(sm);
    let app_clone = app.clone();
    let reg_clone = Arc::clone(reg);
    let session_id = session.id;
    std::thread::spawn(move || {
        SessionManager::do_spawn(sm_clone, app_clone, session_id, prompt, reg_clone);
    });

    // Poll DB for session completion
    let output = wait_for_session(db, session_id)?;

    db.update_pipeline_step_status(step_id, &PipelineStepStatus::Passed, None)
        .ok();
    emit_pipeline_state(db, app, pipeline_id);

    Ok(output)
}

/// Block until session reaches a terminal status; return concatenated assistant output.
fn wait_for_session(db: &Arc<DatabaseService>, session_id: i64) -> Result<String, String> {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let session = db
            .get_session(session_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("session {session_id} disappeared"))?;

        match session.status {
            SessionStatus::Completed => {
                let lines = db.get_outputs(session_id).unwrap_or_default();
                return Ok(extract_assistant_text(&lines));
            }
            SessionStatus::Stopped | SessionStatus::Error => {
                return Err(format!(
                    "session {session_id} ended with status {:?}",
                    session.status
                ));
            }
            _ => {} // still running
        }
    }
}

/// Concatenate text from assistant-type JSONL lines.
fn extract_assistant_text(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Claude Code stream-json format: {"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
        let Some(content) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// Extract JSON between `<orbit-pipeline-result>` sentinels, if present.
fn parse_pipeline_result(text: &str) -> serde_json::Value {
    const OPEN: &str = "<orbit-pipeline-result>";
    const CLOSE: &str = "</orbit-pipeline-result>";
    if let Some(start) = text.find(OPEN) {
        let after = &text[start + OPEN.len()..];
        if let Some(end) = after.find(CLOSE) {
            let json_str = after[..end].trim();
            if let Ok(v) = serde_json::from_str(json_str) {
                return v;
            }
        }
    }
    serde_json::json!({ "raw": text })
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn advance(
    db: &Arc<DatabaseService>,
    app: &AppHandle,
    id: PipelineId,
    to: PipelineStatus,
) -> Result<(), String> {
    db.update_pipeline_status(id, &to)
        .map_err(|e| e.to_string())?;
    emit_pipeline_state(db, app, id);
    Ok(())
}

fn fail_pipeline(db: &Arc<DatabaseService>, app: &AppHandle, id: PipelineId, reason: &str) {
    eprintln!("[orbit:pipeline] {id} failed: {reason}");
    let _ = db.update_pipeline_status(id, &PipelineStatus::Failed);
    let _ = db.append_pipeline_event(
        id,
        None,
        "failed",
        Some(&serde_json::json!({ "reason": reason })),
    );
    emit_pipeline_state(db, app, id);
}

fn emit_pipeline_state(db: &Arc<DatabaseService>, app: &AppHandle, id: PipelineId) {
    if let Ok(Some(p)) = db.get_pipeline(id) {
        let _ = app.emit(
            "pipeline:state",
            &serde_json::json!({
                "pipelineId": p.id,
                "status": p.status,
                "steps": p.steps,
            }),
        );
    }
}

// ── Prompt builders ──────────────────────────────────────────────────────────

fn build_planner_prompt(pipeline: &Pipeline) -> String {
    format!(
        r#"You are the Planner agent in an Orbit-mode multi-agent development pipeline.

Your task is to analyze the following user request and produce a detailed implementation plan.

## User Request
{user_request}

## Working Directory
{cwd}

## Instructions
1. Analyze the request thoroughly
2. Break it down into concrete implementation steps
3. Identify files to create or modify
4. Consider edge cases and potential issues
5. Produce your plan

**IMPORTANT**: End your response with your structured plan in this exact format:

<orbit-pipeline-result>
{{
  "approved": true,
  "summary": "One-line summary of what will be built",
  "steps": ["step 1", "step 2", "step 3"],
  "files_to_touch": ["path/to/file1", "path/to/file2"],
  "notes": "Any important implementation notes"
}}
</orbit-pipeline-result>
"#,
        user_request = pipeline.user_request,
        cwd = pipeline.worktree_path.as_deref().unwrap_or(".")
    )
}

fn build_developer_prompt(pipeline: &Pipeline, plan_text: &str) -> String {
    format!(
        r#"You are the Developer agent in an Orbit-mode multi-agent development pipeline.

The Planner has produced the following implementation plan. Your job is to implement it.

## Original Request
{user_request}

## Implementation Plan
{plan}

## Working Directory
{cwd}

## Instructions
1. Follow the plan step by step
2. Write clean, well-structured code
3. Run tests if applicable
4. Commit your changes when done

Implement the plan now.
"#,
        user_request = pipeline.user_request,
        plan = plan_text,
        cwd = pipeline.worktree_path.as_deref().unwrap_or(".")
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use crate::pipeline::models::{PipelineAgentConfig, PipelineConfig, PipelineLimits};
    use crate::providers::ProviderRegistry;
    use crate::services::database::DatabaseService;
    use crate::services::session_manager::SessionManager;

    use super::*;

    fn make_orch() -> PipelineOrchestrator {
        let db = Arc::new(DatabaseService::open_in_memory().unwrap());
        let sm = Arc::new(RwLock::new(SessionManager::new(Arc::clone(&db))));
        let reg = Arc::new(ProviderRegistry::with_shipped_providers());
        PipelineOrchestrator::new(db, sm, reg)
    }

    fn default_config() -> PipelineConfig {
        let agent = |p: &str| PipelineAgentConfig {
            provider: p.to_string(),
            model: "auto".to_string(),
        };
        PipelineConfig {
            planner: agent("claude-code"),
            developer: agent("claude-code"),
            reviewer: agent("claude-code"),
            tester: agent("claude-code"),
            limits: PipelineLimits::default(),
            require_final_review: false,
            require_tests: false,
            quality_commands: vec![],
        }
    }

    // ── extract_assistant_text ───────────────────────────────────────────────

    #[test]
    fn extracts_text_from_stream_json_assistant_message() {
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
        let result = extract_assistant_text(&[line.to_string()]);
        assert!(result.contains("Hello world"));
    }

    #[test]
    fn skips_non_assistant_jsonl_lines() {
        let lines = vec![
            r#"{"type":"system","text":"init"}"#.to_string(),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Answer"}]}}"#
                .to_string(),
            r#"not json at all"#.to_string(),
        ];
        let result = extract_assistant_text(&lines);
        assert!(result.contains("Answer"));
        assert!(!result.contains("init"));
    }

    #[test]
    fn concatenates_multiple_text_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Part1"},{"type":"text","text":"Part2"}]}}"#;
        let result = extract_assistant_text(&[line.to_string()]);
        assert!(result.contains("Part1"));
        assert!(result.contains("Part2"));
    }

    #[test]
    fn returns_empty_string_for_empty_input() {
        assert_eq!(extract_assistant_text(&[]), "");
    }

    // ── parse_pipeline_result ────────────────────────────────────────────────

    #[test]
    fn extracts_json_inside_sentinel() {
        let text = r#"Here is my plan.
<orbit-pipeline-result>
{"approved": true, "summary": "Build feature X"}
</orbit-pipeline-result>
Done."#;
        let v = parse_pipeline_result(text);
        assert_eq!(v["approved"], true);
        assert_eq!(v["summary"], "Build feature X");
    }

    #[test]
    fn falls_back_to_raw_when_no_sentinel() {
        let text = "Here is my plan without a sentinel.";
        let v = parse_pipeline_result(text);
        assert_eq!(v["raw"], text);
    }

    #[test]
    fn falls_back_to_raw_when_sentinel_has_invalid_json() {
        let text = "<orbit-pipeline-result>not json</orbit-pipeline-result>";
        let v = parse_pipeline_result(text);
        assert!(v["raw"].as_str().unwrap().contains("not json"));
    }

    #[test]
    fn sentinel_may_appear_after_preamble_text() {
        let text = "Lots of analysis...\n<orbit-pipeline-result>\n{\"steps\":[]}\n</orbit-pipeline-result>";
        let v = parse_pipeline_result(text);
        assert!(v["steps"].is_array());
    }

    // ── build prompts ────────────────────────────────────────────────────────

    #[test]
    fn planner_prompt_contains_user_request() {
        let orch = make_orch();
        let req = CreatePipelineRequest {
            name: "test".into(),
            user_request: "Add login page".into(),
            worktree_path: "/repo".into(),
            config: default_config(),
        };
        let pipeline = orch.create(req).unwrap();
        let prompt = build_planner_prompt(&pipeline);
        assert!(prompt.contains("Add login page"));
        assert!(prompt.contains("<orbit-pipeline-result>"));
    }

    #[test]
    fn developer_prompt_contains_plan_and_request() {
        let orch = make_orch();
        let req = CreatePipelineRequest {
            name: "test".into(),
            user_request: "Add login page".into(),
            worktree_path: "/repo".into(),
            config: default_config(),
        };
        let pipeline = orch.create(req).unwrap();
        let prompt = build_developer_prompt(&pipeline, "Step 1: Create component");
        assert!(prompt.contains("Add login page"));
        assert!(prompt.contains("Step 1: Create component"));
    }

    // ── orchestrator.create ──────────────────────────────────────────────────

    #[test]
    fn create_returns_pipeline_with_steps() {
        let orch = make_orch();
        let req = CreatePipelineRequest {
            name: "My pipeline".into(),
            user_request: "Implement feature X".into(),
            worktree_path: "/tmp/repo".into(),
            config: default_config(),
        };
        let p = orch.create(req).unwrap();
        assert_eq!(p.name, "My pipeline");
        assert_eq!(p.status, PipelineStatus::Created);
        // 4 standard roles: Planner, Developer, Reviewer, Tester
        assert_eq!(p.steps.len(), 4);
    }

    #[test]
    fn create_with_final_review_adds_fifth_step() {
        let orch = make_orch();
        let mut config = default_config();
        config.require_final_review = true;
        let req = CreatePipelineRequest {
            name: "Full pipeline".into(),
            user_request: "Implement Y".into(),
            worktree_path: "/tmp/repo".into(),
            config,
        };
        let p = orch.create(req).unwrap();
        assert_eq!(p.steps.len(), 5);
    }

    #[test]
    fn transition_advances_pipeline_status() {
        // transition() needs an AppHandle which requires a running Tauri app,
        // so we test the DB layer directly here and let state_machine tests
        // cover the logic.
        let orch = make_orch();
        let req = CreatePipelineRequest {
            name: "t".into(),
            user_request: "q".into(),
            worktree_path: "/r".into(),
            config: default_config(),
        };
        let p = orch.create(req).unwrap();
        assert_eq!(p.status, PipelineStatus::Created);
        // Verify DB write works (transition needs AppHandle so we call DB directly)
        orch.db
            .update_pipeline_status(p.id, &PipelineStatus::Preflight)
            .unwrap();
        let updated = orch.db.get_pipeline(p.id).unwrap().unwrap();
        assert_eq!(updated.status, PipelineStatus::Preflight);
    }
}
