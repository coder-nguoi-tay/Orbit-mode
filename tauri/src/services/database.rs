use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::models::{AccountAuthType, AccountStatus, Project, ProviderAccount, Session, SessionId};

enum WorkerMsg {
    Row(SessionId, String),
    Flush(std::sync::mpsc::SyncSender<()>),
}

pub struct DatabaseService {
    conn: Arc<Mutex<Connection>>,
    output_tx: std::sync::mpsc::SyncSender<WorkerMsg>,
}

fn flush_batch(conn: &Mutex<Connection>, buf: &mut Vec<(SessionId, String)>) {
    if buf.is_empty() {
        return;
    }
    let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = conn.execute_batch("BEGIN") {
        eprintln!("[orbit] flush_batch: BEGIN failed: {e}");
        buf.clear();
        return;
    }
    for (session_id, data) in buf.drain(..) {
        if let Err(e) = conn.execute(
            "INSERT INTO session_outputs (session_id, data) VALUES (?1, ?2)",
            rusqlite::params![session_id, data],
        ) {
            eprintln!("[orbit] flush_batch: INSERT failed for session {session_id}: {e}");
        }
    }
    if let Err(e) = conn.execute_batch("COMMIT") {
        eprintln!("[orbit] flush_batch: COMMIT failed: {e}");
    }
}

fn start_output_worker(conn: Arc<Mutex<Connection>>) -> std::sync::mpsc::SyncSender<WorkerMsg> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<WorkerMsg>(1024);
    std::thread::spawn(move || {
        let mut buf: Vec<(SessionId, String)> = Vec::with_capacity(64);
        loop {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match rx.recv_timeout(remaining) {
                    Ok(WorkerMsg::Row(sid, data)) => buf.push((sid, data)),
                    Ok(WorkerMsg::Flush(reply)) => {
                        flush_batch(&conn, &mut buf);
                        let _ = reply.send(());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        flush_batch(&conn, &mut buf);
                        return;
                    }
                }
            }
            if !buf.is_empty() {
                flush_batch(&conn, &mut buf);
            }
        }
    });
    tx
}

impl DatabaseService {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Arc::new(Mutex::new(Connection::open(path)?));
        let output_tx = start_output_worker(Arc::clone(&conn));
        let db = DatabaseService { conn, output_tx };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory()?));
        let output_tx = start_output_worker(Arc::clone(&conn));
        let db = DatabaseService { conn, output_tx };
        db.migrate()?;
        Ok(db)
    }

    /// Upgrade local SQLite tables while preserving legacy sessions and credentials.
    ///
    /// @return Success after all compatible schema changes are applied.
    /// @throws rusqlite::Error If a required table or index cannot be created.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    fn migrate(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        // Run schema migrations (errors ignored — column may already exist)
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN claude_session_id TEXT");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN cwd TEXT");
        let _ = conn
            .execute_batch("ALTER TABLE sessions ADD COLUMN provider TEXT DEFAULT 'claude-code'");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN ssh_host TEXT");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN ssh_user TEXT");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN api_key_enc TEXT");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN ssh_password_enc TEXT");
        let _ = conn
            .execute_batch("ALTER TABLE sessions ADD COLUMN skip_permissions INTEGER DEFAULT 1");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN parent_session_id INTEGER");
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN depth INTEGER DEFAULT 0");

        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_keys (
                provider_id TEXT NOT NULL PRIMARY KEY,
                env_var     TEXT NOT NULL,
                key_enc     TEXT NOT NULL,
                updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        );

        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS api_keys (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                key_hash   TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        );

        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS http_settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        );

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                path       TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id        INTEGER REFERENCES projects(id),
                name              TEXT,
                status            TEXT NOT NULL DEFAULT 'initializing',
                worktree_path     TEXT,
                branch_name       TEXT,
                permission_mode   TEXT NOT NULL DEFAULT 'ignore',
                model             TEXT,
                pid               INTEGER,
                cwd               TEXT,
                claude_session_id TEXT,
                created_at        TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
                provider          TEXT DEFAULT 'claude-code',
                provider_account_id TEXT,
                ssh_host          TEXT,
                ssh_user          TEXT,
                skip_permissions  INTEGER DEFAULT 1,
                parent_session_id INTEGER REFERENCES sessions(id),
                depth             INTEGER DEFAULT 0
            );
            -- Add claude_session_id column if upgrading from older schema
            CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY);

            CREATE TABLE IF NOT EXISTS session_outputs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL REFERENCES sessions(id),
                data       TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_session_outputs_session_id
                ON session_outputs(session_id);

            CREATE TABLE IF NOT EXISTS session_usage_snapshots (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id          INTEGER NOT NULL REFERENCES sessions(id),
                provider            TEXT NOT NULL,
                model               TEXT,
                input_tokens        INTEGER NOT NULL DEFAULT 0,
                output_tokens       INTEGER NOT NULL DEFAULT 0,
                cached_input_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens  INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens    INTEGER NOT NULL DEFAULT 0,
                total_tokens        INTEGER NOT NULL DEFAULT 0,
                context_tokens      INTEGER,
                context_limit       INTEGER,
                context_percent     REAL,
                estimated_cost_usd  REAL,
                created_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_session_usage_session_id
                ON session_usage_snapshots(session_id);
            CREATE INDEX IF NOT EXISTS idx_session_usage_created_at
                ON session_usage_snapshots(created_at);

            CREATE TABLE IF NOT EXISTS provider_quota_snapshots (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                provider            TEXT NOT NULL,
                account_key         TEXT NOT NULL DEFAULT 'default',
                provider_account_id TEXT,
                window_type         TEXT NOT NULL,
                utilization         REAL NOT NULL,
                reset_at            INTEGER,
                status              TEXT,
                source              TEXT NOT NULL,
                created_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_provider_quota_provider
                ON provider_quota_snapshots(provider);
            CREATE INDEX IF NOT EXISTS idx_provider_quota_created_at
                ON provider_quota_snapshots(created_at);

            CREATE TABLE IF NOT EXISTS provider_accounts (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                label TEXT NOT NULL,
                auth_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'unknown',
                execution_scope TEXT NOT NULL DEFAULT 'local',
                profile_home TEXT,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_used_at TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_account_default
                ON provider_accounts(provider_id, execution_scope) WHERE is_default = 1;
            CREATE TABLE IF NOT EXISTS project_provider_accounts (
                project_id INTEGER NOT NULL REFERENCES projects(id),
                provider_id TEXT NOT NULL,
                account_id TEXT NOT NULL REFERENCES provider_accounts(id),
                PRIMARY KEY (project_id, provider_id)
            );
            CREATE TABLE IF NOT EXISTS provider_account_fallbacks (
                source_account_id TEXT PRIMARY KEY REFERENCES provider_accounts(id),
                target_account_id TEXT NOT NULL REFERENCES provider_accounts(id),
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS provider_account_auto_pool (
                account_id TEXT PRIMARY KEY REFERENCES provider_accounts(id),
                enabled INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS session_account_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL REFERENCES sessions(id),
                account_id TEXT NOT NULL,
                event TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
        ",
        )?;
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN provider_account_id TEXT");
        let _ = conn.execute_batch(
            "ALTER TABLE provider_quota_snapshots ADD COLUMN provider_account_id TEXT",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE session_usage_snapshots ADD COLUMN provider_account_id TEXT",
        );
        conn.execute(
            "INSERT OR IGNORE INTO provider_accounts
             (id, provider_id, label, auth_type, status, execution_scope, is_default)
             VALUES ('codex-system-default', 'codex', 'System Default',
                     'chat_gpt_authenticated', 'unknown', 'local', 1)",
            [],
        )?;

        // Pipeline tables
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pipelines (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id       INTEGER REFERENCES projects(id),
                name             TEXT NOT NULL,
                user_request     TEXT NOT NULL,
                worktree_path    TEXT,
                status           TEXT NOT NULL DEFAULT 'created',
                config_json      TEXT NOT NULL DEFAULT '{}',
                baseline_git_head TEXT,
                review_loops     INTEGER NOT NULL DEFAULT 0,
                test_loops       INTEGER NOT NULL DEFAULT 0,
                plan_revisions   INTEGER NOT NULL DEFAULT 0,
                total_agent_runs INTEGER NOT NULL DEFAULT 0,
                created_at       TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
                completed_at     TEXT
            );

            CREATE TABLE IF NOT EXISTS pipeline_steps (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_id  INTEGER NOT NULL REFERENCES pipelines(id),
                role         TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'pending',
                provider_id  TEXT NOT NULL,
                model        TEXT,
                attempt      INTEGER NOT NULL DEFAULT 1,
                started_at   TEXT,
                completed_at TEXT,
                error        TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_pipeline_steps_pipeline_id
                ON pipeline_steps(pipeline_id);

            CREATE TABLE IF NOT EXISTS pipeline_step_sessions (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_step_id INTEGER NOT NULL REFERENCES pipeline_steps(id),
                session_id       INTEGER NOT NULL REFERENCES sessions(id),
                attempt          INTEGER NOT NULL DEFAULT 1,
                created_at       TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS pipeline_artifacts (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_id   INTEGER NOT NULL REFERENCES pipelines(id),
                step_id       INTEGER REFERENCES pipeline_steps(id),
                artifact_type TEXT NOT NULL,
                version       INTEGER NOT NULL DEFAULT 1,
                content_json  TEXT NOT NULL,
                created_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS pipeline_events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_id  INTEGER NOT NULL REFERENCES pipelines(id),
                step_id      INTEGER REFERENCES pipeline_steps(id),
                event_type   TEXT NOT NULL,
                payload_json TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_pipeline_events_pipeline_id
                ON pipeline_events(pipeline_id);
            ",
        )?;

        Ok(())
    }

    // ── Pipeline CRUD ──────────────────────────────────────────────────────────

    pub fn create_pipeline(
        &self,
        name: &str,
        user_request: &str,
        worktree_path: Option<&str>,
        config: &crate::pipeline::models::PipelineConfig,
    ) -> SqlResult<crate::pipeline::models::PipelineId> {
        let config_json = serde_json::to_string(config).unwrap_or_default();
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO pipelines (name, user_request, worktree_path, status, config_json)
             VALUES (?1, ?2, ?3, 'created', ?4)",
            params![name, user_request, worktree_path, config_json],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_pipeline(
        &self,
        id: crate::pipeline::models::PipelineId,
    ) -> SqlResult<Option<crate::pipeline::models::Pipeline>> {
        use crate::pipeline::models::{Pipeline, PipelineConfig, PipelineStatus};
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let row = conn
            .query_row(
                "SELECT id, project_id, name, user_request, worktree_path, status, config_json,
                        baseline_git_head, review_loops, test_loops, plan_revisions,
                        total_agent_runs, created_at, updated_at, completed_at
                 FROM pipelines WHERE id = ?1",
                params![id],
                |row| {
                    let status_str: String = row.get(5)?;
                    let config_json: String = row.get(6)?;
                    let config: PipelineConfig = serde_json::from_str(&config_json)
                        .unwrap_or_else(|_| PipelineConfig::default_preset("claude-code", "auto"));
                    Ok(Pipeline {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        name: row.get(2)?,
                        user_request: row.get(3)?,
                        worktree_path: row.get(4)?,
                        status: status_str.parse().unwrap_or(PipelineStatus::Created),
                        config,
                        baseline_git_head: row.get(7)?,
                        review_loops: row.get::<_, i64>(8)? as u32,
                        test_loops: row.get::<_, i64>(9)? as u32,
                        plan_revisions: row.get::<_, i64>(10)? as u32,
                        total_agent_runs: row.get::<_, i64>(11)? as u32,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        completed_at: row.get(14)?,
                        steps: vec![],
                    })
                },
            )
            .optional()?;
        if let Some(mut p) = row {
            p.steps = self.get_pipeline_steps_conn(&conn, id)?;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }

    pub fn list_pipelines(&self) -> SqlResult<Vec<crate::pipeline::models::Pipeline>> {
        use crate::pipeline::models::{Pipeline, PipelineConfig, PipelineStatus};
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, user_request, worktree_path, status, config_json,
                    baseline_git_head, review_loops, test_loops, plan_revisions,
                    total_agent_runs, created_at, updated_at, completed_at
             FROM pipelines ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(5)?;
                let config_json: String = row.get(6)?;
                let config: PipelineConfig = serde_json::from_str(&config_json)
                    .unwrap_or_else(|_| PipelineConfig::default_preset("claude-code", "auto"));
                Ok(Pipeline {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    user_request: row.get(3)?,
                    worktree_path: row.get(4)?,
                    status: status_str.parse().unwrap_or(PipelineStatus::Created),
                    config,
                    baseline_git_head: row.get(7)?,
                    review_loops: row.get::<_, i64>(8)? as u32,
                    test_loops: row.get::<_, i64>(9)? as u32,
                    plan_revisions: row.get::<_, i64>(10)? as u32,
                    total_agent_runs: row.get::<_, i64>(11)? as u32,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    completed_at: row.get(14)?,
                    steps: vec![],
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        rows.into_iter()
            .map(|mut p| {
                p.steps = self.get_pipeline_steps_conn(&conn, p.id)?;
                Ok(p)
            })
            .collect()
    }

    pub fn update_pipeline_status(
        &self,
        id: crate::pipeline::models::PipelineId,
        status: &crate::pipeline::models::PipelineStatus,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE pipelines SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![status.to_string(), id],
        )?;
        Ok(())
    }

    pub fn set_pipeline_baseline(
        &self,
        id: crate::pipeline::models::PipelineId,
        git_head: &str,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE pipelines SET baseline_git_head = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![git_head, id],
        )?;
        Ok(())
    }

    pub fn create_pipeline_step(
        &self,
        pipeline_id: crate::pipeline::models::PipelineId,
        role: &crate::pipeline::models::PipelineAgentRole,
        provider_id: &str,
        model: Option<&str>,
    ) -> SqlResult<crate::pipeline::models::PipelineStepId> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO pipeline_steps (pipeline_id, role, status, provider_id, model, attempt)
             VALUES (?1, ?2, 'pending', ?3, ?4, 1)",
            params![pipeline_id, role.to_string(), provider_id, model],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_pipeline_step_status(
        &self,
        step_id: crate::pipeline::models::PipelineStepId,
        status: &crate::pipeline::models::PipelineStepStatus,
        error: Option<&str>,
    ) -> SqlResult<()> {
        use crate::pipeline::models::PipelineStepStatus;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let started_sql = match status {
            PipelineStepStatus::Running | PipelineStepStatus::Starting => {
                "started_at = datetime('now'),"
            }
            _ => "",
        };
        let completed_sql = match status {
            PipelineStepStatus::Passed
            | PipelineStepStatus::Failed
            | PipelineStepStatus::NeedsChanges
            | PipelineStepStatus::Cancelled
            | PipelineStepStatus::Skipped => "completed_at = datetime('now'),",
            _ => "",
        };
        let sql = format!(
            "UPDATE pipeline_steps SET status = ?1, {started_sql} {completed_sql} error = ?2 WHERE id = ?3"
        );
        conn.execute(&sql, params![status.to_string(), error, step_id])?;
        Ok(())
    }

    pub fn link_step_session(
        &self,
        step_id: crate::pipeline::models::PipelineStepId,
        session_id: i64,
        attempt: i64,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO pipeline_step_sessions (pipeline_step_id, session_id, attempt)
             VALUES (?1, ?2, ?3)",
            params![step_id, session_id, attempt],
        )?;
        Ok(())
    }

    pub fn save_pipeline_artifact(
        &self,
        pipeline_id: crate::pipeline::models::PipelineId,
        step_id: Option<crate::pipeline::models::PipelineStepId>,
        artifact_type: &str,
        content: &serde_json::Value,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM pipeline_artifacts
             WHERE pipeline_id = ?1 AND artifact_type = ?2",
            params![pipeline_id, artifact_type],
            |row| row.get(0),
        )?;
        let content_json = serde_json::to_string(content).unwrap_or_default();
        conn.execute(
            "INSERT INTO pipeline_artifacts (pipeline_id, step_id, artifact_type, version, content_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![pipeline_id, step_id, artifact_type, version + 1, content_json],
        )?;
        Ok(())
    }

    pub fn append_pipeline_event(
        &self,
        pipeline_id: crate::pipeline::models::PipelineId,
        step_id: Option<crate::pipeline::models::PipelineStepId>,
        event_type: &str,
        payload: Option<&serde_json::Value>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let payload_json = payload.map(|p| serde_json::to_string(p).unwrap_or_default());
        conn.execute(
            "INSERT INTO pipeline_events (pipeline_id, step_id, event_type, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![pipeline_id, step_id, event_type, payload_json],
        )?;
        Ok(())
    }

    fn get_pipeline_steps_conn(
        &self,
        conn: &Connection,
        pipeline_id: crate::pipeline::models::PipelineId,
    ) -> SqlResult<Vec<crate::pipeline::models::PipelineStep>> {
        use crate::pipeline::models::{PipelineAgentRole, PipelineStep, PipelineStepStatus};
        let mut stmt = conn.prepare(
            "SELECT ps.id, ps.pipeline_id, ps.role, ps.status, ps.provider_id, ps.model,
                    ps.attempt, ps.started_at, ps.completed_at, ps.error,
                    pss.session_id
             FROM pipeline_steps ps
             LEFT JOIN pipeline_step_sessions pss
                 ON pss.pipeline_step_id = ps.id AND pss.attempt = ps.attempt
             WHERE ps.pipeline_id = ?1
             ORDER BY ps.id",
        )?;
        stmt.query_map(params![pipeline_id], |row| {
            let role_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            Ok(PipelineStep {
                id: row.get(0)?,
                pipeline_id: row.get(1)?,
                role: role_str.parse().unwrap_or(PipelineAgentRole::Planner),
                status: status_str.parse().unwrap_or(PipelineStepStatus::Pending),
                provider_id: row.get(4)?,
                model: row.get(5)?,
                attempt: row.get(6)?,
                started_at: row.get(7)?,
                completed_at: row.get(8)?,
                error: row.get(9)?,
                session_id: row.get(10)?,
            })
        })
        .and_then(|rows| rows.collect())
    }

    pub fn create_project(&self, name: &str, path: &str) -> SqlResult<Project> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR IGNORE INTO projects (name, path) VALUES (?1, ?2)",
            params![name, path],
        )?;
        let project = conn.query_row(
            "SELECT id, name, path, created_at FROM projects WHERE path = ?1",
            params![path],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )?;
        Ok(project)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &self,
        project_id: Option<i64>,
        name: Option<&str>,
        cwd: &str,
        permission_mode: &str,
        model: Option<&str>,
        provider: Option<&str>,
        ssh_host: Option<&str>,
        ssh_user: Option<&str>,
    ) -> SqlResult<SessionId> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO sessions (project_id, name, cwd, status, permission_mode, model, provider, ssh_host, ssh_user)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                project_id,
                name,
                cwd,
                crate::models::SessionStatus::Initializing,
                permission_mode,
                model,
                provider.unwrap_or("claude-code"),
                ssh_host,
                ssh_user,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_session_parent(
        &self,
        id: SessionId,
        parent_id: SessionId,
        depth: i32,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET parent_session_id = ?1, depth = ?2 WHERE id = ?3",
            params![parent_id, depth, id],
        )?;
        Ok(())
    }

    pub fn update_session_status(
        &self,
        id: SessionId,
        status: crate::models::SessionStatus,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn update_session_pid(&self, id: SessionId, pid: i32) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET pid = ?1, status = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![pid, crate::models::SessionStatus::Running, id],
        )?;
        Ok(())
    }

    pub fn update_session_model(&self, id: SessionId, model: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET model = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![model, id],
        )?;
        Ok(())
    }

    /// Store encrypted API key and/or SSH password for a session.
    pub fn save_session_secrets(
        &self,
        id: SessionId,
        api_key: Option<&str>,
        ssh_password: Option<&str>,
    ) -> SqlResult<()> {
        let api_enc = api_key.and_then(|k| crate::services::crypto::encrypt(k).ok());
        let pw_enc = ssh_password.and_then(|p| crate::services::crypto::encrypt(p).ok());
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET api_key_enc = ?1, ssh_password_enc = ?2, \
             updated_at = datetime('now') WHERE id = ?3",
            params![api_enc, pw_enc, id],
        )?;
        Ok(())
    }

    /// Load and decrypt API key and SSH password for a session.
    pub fn load_session_secrets(
        &self,
        id: SessionId,
    ) -> SqlResult<(Option<String>, Option<String>)> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT api_key_enc, ssh_password_enc FROM sessions WHERE id = ?1")?;
        let result = stmt
            .query_row(params![id], |row| {
                let api_enc: Option<String> = row.get(0)?;
                let pw_enc: Option<String> = row.get(1)?;
                Ok((api_enc, pw_enc))
            })
            .optional()?;

        match result {
            Some((api_enc, pw_enc)) => {
                let api_key = api_enc.and_then(|e| crate::services::crypto::decrypt(&e).ok());
                let ssh_pw = pw_enc.and_then(|e| crate::services::crypto::decrypt(&e).ok());
                Ok((api_key, ssh_pw))
            }
            None => Ok((None, None)),
        }
    }

    /// Save an API key for a provider (upsert). The key is encrypted before storage.
    pub fn save_provider_key(
        &self,
        provider_id: &str,
        env_var: &str,
        api_key: &str,
    ) -> SqlResult<()> {
        let key_enc = crate::services::crypto::encrypt(api_key).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e)))
        })?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO provider_keys (provider_id, env_var, key_enc, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now')) \
             ON CONFLICT(provider_id) DO UPDATE SET env_var=?2, key_enc=?3, updated_at=datetime('now')",
            params![provider_id, env_var, key_enc],
        )?;
        Ok(())
    }

    /// Load the decrypted API key for a provider. Returns (env_var, api_key) or None.
    pub fn load_provider_key(&self, provider_id: &str) -> SqlResult<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT env_var, key_enc FROM provider_keys WHERE provider_id = ?1")?;
        let result = stmt
            .query_row(params![provider_id], |row| {
                let env_var: String = row.get(0)?;
                let key_enc: String = row.get(1)?;
                Ok((env_var, key_enc))
            })
            .optional()?;

        match result {
            Some((env_var, key_enc)) => {
                let api_key = crate::services::crypto::decrypt(&key_enc).map_err(|e| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e)))
                })?;
                Ok(Some((env_var, api_key)))
            }
            None => Ok(None),
        }
    }

    /// List all provider IDs that have saved keys. Returns whether each is configured.
    pub fn list_provider_keys(&self) -> SqlResult<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT provider_id, env_var FROM provider_keys")?;
        let rows = stmt.query_map(params![], |row| {
            let provider_id: String = row.get(0)?;
            let env_var: String = row.get(1)?;
            Ok((provider_id, env_var))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Delete a saved provider key.
    pub fn delete_provider_key(&self, provider_id: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM provider_keys WHERE provider_id = ?1",
            params![provider_id],
        )?;
        Ok(())
    }

    pub fn update_session_worktree(
        &self,
        id: SessionId,
        worktree_path: &str,
        branch_name: &str,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .execute(
                "UPDATE sessions SET worktree_path = ?1, branch_name = ?2, \
             updated_at = datetime('now') WHERE id = ?3",
                params![worktree_path, branch_name, id],
            )?;
        Ok(())
    }

    pub fn get_sessions(&self) -> SqlResult<Vec<Session>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, status, worktree_path, branch_name,
                    permission_mode, model, pid, cwd, created_at, updated_at, provider,
                    ssh_host, ssh_user, skip_permissions, parent_session_id, depth,
                    provider_account_id
             FROM sessions ORDER BY created_at DESC",
        )?;
        let sessions = stmt
            .query_map([], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    status: row.get(3)?,
                    worktree_path: row.get(4)?,
                    branch_name: row.get(5)?,
                    permission_mode: row.get(6)?,
                    model: row.get(7)?,
                    pid: row.get(8)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    cwd: row.get(9)?,
                    provider: row
                        .get::<_, Option<String>>(12)?
                        .unwrap_or_else(|| "claude-code".to_string()),
                    provider_account_id: row.get(18)?,
                    project_name: None,
                    git_branch: None,
                    tokens: None,
                    context_percent: None,
                    pending_approval: None,
                    mini_log: None,
                    ssh_host: row.get(13)?,
                    ssh_user: row.get(14)?,
                    attention: None,
                    skip_permissions: row
                        .get::<_, Option<bool>>(15)
                        .ok()
                        .flatten()
                        .unwrap_or(true),
                    parent_session_id: row.get(16).ok().flatten(),
                    depth: row.get::<_, Option<i32>>(17).ok().flatten().unwrap_or(0),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(sessions)
    }

    pub fn get_session(&self, id: SessionId) -> SqlResult<Option<Session>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, status, worktree_path, branch_name,
                    permission_mode, model, pid, cwd, created_at, updated_at,
                    provider, ssh_host, ssh_user, skip_permissions, parent_session_id,
                    depth, provider_account_id
             FROM sessions WHERE id = ?1",
        )?;
        let session = stmt
            .query_row(params![id], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    status: row.get(3)?,
                    worktree_path: row.get(4)?,
                    branch_name: row.get(5)?,
                    permission_mode: row.get(6)?,
                    model: row.get(7)?,
                    provider: row
                        .get::<_, Option<String>>(12)?
                        .unwrap_or_else(|| "claude-code".to_string()),
                    provider_account_id: row.get(18)?,
                    pid: row.get(8)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    cwd: row.get(9)?,
                    project_name: None,
                    git_branch: None,
                    tokens: None,
                    context_percent: None,
                    pending_approval: None,
                    mini_log: None,
                    ssh_host: row.get(13)?,
                    ssh_user: row.get(14)?,
                    attention: None,
                    skip_permissions: row
                        .get::<_, Option<bool>>(15)
                        .ok()
                        .flatten()
                        .unwrap_or(true),
                    parent_session_id: row.get(16).ok().flatten(),
                    depth: row.get::<_, Option<i32>>(17).ok().flatten().unwrap_or(0),
                })
            })
            .optional()?;
        Ok(session)
    }

    pub fn get_projects(&self) -> SqlResult<Vec<Project>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT id, name, path, created_at FROM projects ORDER BY name ASC")?;
        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(projects)
    }

    pub fn insert_output(&self, session_id: SessionId, data: &str) -> SqlResult<()> {
        let _ = self
            .output_tx
            .send(WorkerMsg::Row(session_id, data.to_string()));
        Ok(())
    }

    /// Block until all pending output rows are written. Required before calling get_outputs in tests.
    pub fn flush_outputs(&self) {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        let _ = self.output_tx.send(WorkerMsg::Flush(reply_tx));
        let _ = reply_rx.recv();
    }

    pub fn update_claude_session_id(&self, id: SessionId, claude_id: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET claude_session_id = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![claude_id, id],
        )?;
        Ok(())
    }

    pub fn get_claude_session_id(&self, id: SessionId) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let result = conn
            .query_row(
                "SELECT claude_session_id FROM sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn rename_session(&self, id: SessionId, name: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: SessionId) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch("BEGIN")?;
        conn.execute(
            "DELETE FROM session_outputs WHERE session_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn delete_all_sessions(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch("PRAGMA foreign_keys = OFF")?;
        conn.execute_batch("DELETE FROM session_outputs")?;
        conn.execute_batch("DELETE FROM sessions")?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        Ok(())
    }

    pub fn get_outputs(&self, session_id: SessionId) -> SqlResult<Vec<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT data FROM session_outputs WHERE session_id = ?1 ORDER BY id ASC")?;
        let rows = stmt
            .query_map(params![session_id], |row| row.get(0))?
            .collect::<SqlResult<Vec<String>>>()?;
        Ok(rows)
    }

    // ── API key management ──────────────────────────────────────────

    pub fn create_api_key(&self, id: &str, label: &str, key_hash: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO api_keys (id, label, key_hash) VALUES (?1, ?2, ?3)",
            params![id, label, key_hash],
        )?;
        Ok(())
    }

    pub fn list_api_keys(&self) -> SqlResult<Vec<(String, String, String)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT id, label, created_at FROM api_keys ORDER BY created_at DESC")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_api_key(&self, id: &str) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count = conn.execute("DELETE FROM api_keys WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    pub fn validate_api_key_hash(&self, key_hash: &str) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_hash = ?1)",
                params![key_hash],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(exists)
    }

    // ── HTTP server settings ────────────────────────────────────────

    pub fn get_http_setting(&self, key: &str) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT value FROM http_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
    }

    pub fn set_http_setting(&self, key: &str, value: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR REPLACE INTO http_settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ── Usage & Quota snapshots ─────────────────────────────────────

    /// Attribute one session token sample to the provider account that ran it.
    ///
    /// @param session_id Session whose usage is recorded.
    /// @param provider_account_id Account active when the tokens were consumed.
    /// @param provider Provider that emitted the sample.
    /// @param model Model used for cost estimation.
    /// @param usage Normalized token counts.
    /// @param context_percent Share of the model context window in use.
    /// @return Success after the sample is stored.
    /// @throws rusqlite::Error If SQLite cannot write the snapshot.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn record_session_usage(
        &self,
        session_id: SessionId,
        provider_account_id: Option<&str>,
        provider: &str,
        model: Option<&str>,
        usage: &crate::models::TokenUsage,
        context_percent: Option<f64>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let total = if usage.total > 0 {
            usage.total
        } else {
            usage.input + usage.output
        };
        let estimated_cost = usage.estimated_cost.or_else(|| {
            crate::services::pricing::estimate_cost(
                model,
                usage.input,
                usage.output,
                usage.cache_read,
                usage.cache_write,
            )
        });

        conn.execute(
            "INSERT INTO session_usage_snapshots (
                session_id, provider_account_id, provider, model, input_tokens, output_tokens,
                cached_input_tokens, cache_write_tokens, reasoning_tokens, total_tokens,
                context_tokens, context_limit, context_percent, estimated_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                session_id,
                provider_account_id,
                provider,
                model,
                usage.input as i64,
                usage.output as i64,
                usage.cache_read as i64,
                usage.cache_write as i64,
                usage.reasoning as i64,
                total as i64,
                usage.context_tokens.map(|v| v as i64),
                usage.context_limit.map(|v| v as i64),
                context_percent,
                estimated_cost,
            ],
        )?;
        Ok(())
    }

    /// Store each quota window under its own provider account.
    ///
    /// @param quota Provider-reported account quota sample.
    /// @return Success after all present windows are stored.
    /// @throws rusqlite::Error If SQLite cannot write a window.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn record_provider_quota(&self, quota: &crate::models::ProviderQuota) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref five) = quota.five_hour {
            conn.execute(
                "INSERT INTO provider_quota_snapshots (provider, account_key, provider_account_id, window_type, utilization, reset_at, status, source)
                 VALUES (?1, ?2, ?3, 'five_hour', ?4, ?5, ?6, ?7)",
                params![quota.provider, quota.account_key, quota.provider_account_id, five.utilization, five.resets_at, five.status, quota.source],
            )?;
        }
        if let Some(ref seven) = quota.seven_day {
            conn.execute(
                "INSERT INTO provider_quota_snapshots (provider, account_key, provider_account_id, window_type, utilization, reset_at, status, source)
                 VALUES (?1, ?2, ?3, 'seven_day', ?4, ?5, ?6, ?7)",
                params![quota.provider, quota.account_key, quota.provider_account_id, seven.utilization, seven.resets_at, seven.status, quota.source],
            )?;
        }
        Ok(())
    }

    /// Reconstruct the latest separate windows for every provider account.
    ///
    /// @return Latest known quota for each provider and account key.
    /// @throws rusqlite::Error If SQLite cannot read quota snapshots.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_latest_provider_quotas(&self) -> SqlResult<Vec<crate::models::ProviderQuota>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT provider, account_key, provider_account_id, window_type, utilization, reset_at, status, source, created_at
             FROM provider_quota_snapshots
             ORDER BY id DESC",
        )?;
        let mut map: std::collections::HashMap<(String, String), crate::models::ProviderQuota> =
            std::collections::HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;

        for r in rows {
            let (
                provider,
                account_key,
                provider_account_id,
                window_type,
                utilization,
                reset_at,
                status,
                source,
                created_at,
            ) = r?;
            let entry = map
                .entry((provider.clone(), account_key.clone()))
                .or_insert_with(|| crate::models::ProviderQuota {
                    provider,
                    account_key,
                    provider_account_id,
                    five_hour: None,
                    seven_day: None,
                    updated_at: created_at,
                    source,
                });

            let window = crate::models::QuotaWindow {
                utilization,
                resets_at: reset_at,
                status,
            };

            if window_type == "five_hour" && entry.five_hour.is_none() {
                entry.five_hour = Some(window);
            } else if window_type == "seven_day" && entry.seven_day.is_none() {
                entry.seven_day = Some(window);
            }
        }

        Ok(map.into_values().collect())
    }

    /// Read recent session token samples with the account active at each sample.
    ///
    /// @param limit Maximum number of snapshots to return.
    /// @return Recent account-attributed session usage snapshots.
    /// @throws rusqlite::Error If SQLite cannot read the snapshots.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_session_usages(
        &self,
        limit: usize,
    ) -> SqlResult<Vec<crate::models::SessionUsageSnapshot>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT u.id, u.session_id, s.name, p.name, u.provider, u.model,
                    u.input_tokens, u.output_tokens, u.cached_input_tokens, u.cache_write_tokens,
                    u.reasoning_tokens, u.total_tokens, u.context_tokens, u.context_limit,
                    u.context_percent, u.estimated_cost_usd, s.status, u.created_at,
                    u.provider_account_id
             FROM session_usage_snapshots u
             LEFT JOIN sessions s ON s.id = u.session_id
             LEFT JOIN projects p ON p.id = s.project_id
             ORDER BY u.id DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(crate::models::SessionUsageSnapshot {
                id: row.get(0)?,
                session_id: row.get(1)?,
                session_name: row.get(2)?,
                project_name: row.get(3)?,
                provider: row.get(4)?,
                provider_account_id: row.get(18)?,
                model: row.get(5)?,
                input_tokens: row.get::<_, i64>(6)? as u64,
                output_tokens: row.get::<_, i64>(7)? as u64,
                cached_input_tokens: row.get::<_, i64>(8)? as u64,
                cache_write_tokens: row.get::<_, i64>(9)? as u64,
                reasoning_tokens: row.get::<_, i64>(10)? as u64,
                total_tokens: row.get::<_, i64>(11)? as u64,
                context_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
                context_limit: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
                context_percent: row.get(14)?,
                estimated_cost_usd: row.get(15)?,
                status: row
                    .get::<_, Option<String>>(16)?
                    .unwrap_or_else(|| "idle".into()),
                created_at: row.get(17)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Build the usage dashboard summary from the current SQLite snapshots.
    ///
    /// @return Aggregated token, cost, session and provider-quota data.
    /// @throws rusqlite::Error If SQLite cannot read one of the dashboard queries.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn get_usage_overview(&self) -> SqlResult<crate::models::UsageOverview> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

        // 1. Today's aggregated usage
        let (total_tokens_today, total_cost_today): (i64, f64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(estimated_cost_usd), 0.0)
                 FROM session_usage_snapshots
                 WHERE date(created_at) = date('now')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or((0, 0.0));

        // 2. Active agents count
        let active_agents_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE status = 'running' OR status = 'waiting'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_sessions_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap_or(0);

        // 3. Project breakdown
        let mut proj_stmt = conn.prepare(
            "SELECT COALESCE(p.name, 'Default Project') as pname,
                    SUM(u.total_tokens) as total_tok,
                    SUM(COALESCE(u.estimated_cost_usd, 0.0)) as total_cost,
                    COUNT(DISTINCT u.session_id) as agents
             FROM session_usage_snapshots u
             LEFT JOIN sessions s ON s.id = u.session_id
             LEFT JOIN projects p ON p.id = s.project_id
             GROUP BY pname
             ORDER BY total_tok DESC
             LIMIT 10",
        )?;
        let project_summaries = proj_stmt
            .query_map([], |row| {
                Ok(crate::models::ProjectUsageSummary {
                    project_name: row.get(0)?,
                    total_tokens: row.get::<_, i64>(1)? as u64,
                    estimated_cost_usd: row.get(2)?,
                    agent_count: row.get::<_, i64>(3)? as usize,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()
            .unwrap_or_default();

        // 4. Model breakdown
        let mut model_stmt = conn.prepare(
            "SELECT COALESCE(u.model, 'default') as mname,
                    u.provider,
                    SUM(u.total_tokens) as total_tok,
                    SUM(COALESCE(u.estimated_cost_usd, 0.0)) as total_cost,
                    COUNT(DISTINCT u.session_id) as sess_count
             FROM session_usage_snapshots u
             GROUP BY mname, u.provider
             ORDER BY total_tok DESC
             LIMIT 10",
        )?;
        let model_summaries = model_stmt
            .query_map([], |row| {
                Ok(crate::models::ModelUsageSummary {
                    model: row.get(0)?,
                    provider: row.get(1)?,
                    total_tokens: row.get::<_, i64>(2)? as u64,
                    estimated_cost_usd: row.get(3)?,
                    session_count: row.get::<_, i64>(4)? as usize,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()
            .unwrap_or_default();

        // Release the connection mutex before calling the helper. The helper acquires
        // the same mutex and would otherwise deadlock because Mutex is not re-entrant.
        drop(model_stmt);
        drop(proj_stmt);
        drop(conn);
        let quotas = self.get_latest_provider_quotas().unwrap_or_default();

        Ok(crate::models::UsageOverview {
            total_tokens_today: total_tokens_today as u64,
            total_cost_today,
            active_agents_count: active_agents_count as usize,
            total_sessions_count: total_sessions_count as usize,
            quotas,
            project_summaries,
            model_summaries,
        })
    }
}

const ACCOUNT_COLUMNS: &str =
    "id, provider_id, label, auth_type, status, execution_scope, profile_home,
                              is_default, created_at, updated_at, last_used_at";

/// Decode account metadata without ever loading provider credential files.
///
/// @param row The SQLite account row.
/// @return The provider account metadata.
/// @throws rusqlite::Error If a stored authentication type is unsupported.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-25
fn provider_account_from_row(row: &rusqlite::Row<'_>) -> SqlResult<ProviderAccount> {
    let auth_type: String = row.get(3)?;
    let auth_type = AccountAuthType::from_db(&auth_type).ok_or(rusqlite::Error::InvalidQuery)?;
    let status: String = row.get(4)?;
    Ok(ProviderAccount {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        label: row.get(2)?,
        auth_type,
        status: AccountStatus::from_db(&status),
        execution_scope: row.get(5)?,
        profile_home: row.get(6)?,
        is_default: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_used_at: row.get(10)?,
    })
}

impl DatabaseService {
    /// Save a provider profile containing metadata only.
    ///
    /// @param account The account label, provider, scope and CLI-managed profile path.
    /// @return Success after the profile metadata is persisted.
    /// @throws rusqlite::Error If the account ID already exists or SQLite rejects the row.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn create_provider_account(&self, account: &ProviderAccount) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .execute(
                "INSERT INTO provider_accounts
             (id, provider_id, label, auth_type, status, execution_scope, profile_home, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    account.id,
                    account.provider_id,
                    account.label,
                    account.auth_type.as_str(),
                    account.status.as_str(),
                    account.execution_scope,
                    account.profile_home,
                    account.is_default,
                ],
            )?;
        Ok(())
    }

    /// List configured profiles without loading their credentials.
    ///
    /// @return Account metadata ordered by provider and label.
    /// @throws rusqlite::Error If SQLite cannot read account metadata.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn list_provider_accounts(&self) -> SqlResult<Vec<ProviderAccount>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement = conn.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM provider_accounts ORDER BY provider_id, label"
        ))?;
        let accounts = statement
            .query_map([], provider_account_from_row)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(accounts)
    }

    /// Find the account bound to a session or selected in Settings.
    ///
    /// @param account_id The opaque provider account ID.
    /// @return Account metadata when present.
    /// @throws rusqlite::Error If SQLite cannot read the row.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_provider_account(&self, account_id: &str) -> SqlResult<Option<ProviderAccount>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM provider_accounts WHERE id = ?1"),
            params![account_id],
            provider_account_from_row,
        )
        .optional()
    }

    /// Update a profile's health or authoritative quota state.
    ///
    /// @param account_id The affected account.
    /// @param status The confirmed new state.
    /// @return Success when the update is recorded.
    /// @throws rusqlite::Error If SQLite rejects the update.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn update_provider_account_status(
        &self,
        account_id: &str,
        status: AccountStatus,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .execute(
            "UPDATE provider_accounts SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![status.as_str(), account_id],
        )?;
        Ok(())
    }

    /// Mark paused sessions ready after an official account quota window resets.
    ///
    /// @param account_id The account whose reported quota is available again.
    /// @return Number of sessions that can now be resumed by the user.
    /// @throws rusqlite::Error If SQLite rejects the status update.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn mark_account_sessions_ready(&self, account_id: &str) -> SqlResult<usize> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execute(
                "UPDATE sessions SET status = 'ready_to_resume', updated_at = datetime('now')
             WHERE provider_account_id = ?1 AND status = 'needs_account_action'",
                params![account_id],
            )
    }

    /// Bind a session to the profile that will actually spawn its provider process.
    ///
    /// @param session_id The local Orbit session.
    /// @param account_id The selected account, or None for a legacy session.
    /// @return Success when the binding is persisted.
    /// @throws rusqlite::Error If SQLite rejects the update.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn set_session_provider_account(
        &self,
        session_id: SessionId,
        account_id: Option<&str>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE sessions SET provider_account_id = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![account_id, session_id],
        )?;
        if let Some(account_id) = account_id {
            conn.execute(
                "INSERT INTO session_account_history (session_id, account_id, event)
                 SELECT ?1, ?2, 'selected' WHERE NOT EXISTS (
                   SELECT 1 FROM session_account_history WHERE session_id = ?1 AND event = 'selected'
                 )",
                params![session_id, account_id],
            )?;
        }
        Ok(())
    }

    /// Resolve a project's account override, then its provider default.
    ///
    /// @param project_id The project owning a new session.
    /// @param provider_id The provider selected for that session.
    /// @return The configured account ID, if any.
    /// @throws rusqlite::Error If SQLite cannot read the account settings.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn resolve_default_provider_account(
        &self,
        project_id: i64,
        provider_id: &str,
    ) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let project_account = conn
            .query_row(
                "SELECT account_id FROM project_provider_accounts
                 WHERE project_id = ?1 AND provider_id = ?2",
                params![project_id, provider_id],
                |row| row.get(0),
            )
            .optional()?;
        if project_account.is_some() {
            return Ok(project_account);
        }
        conn.query_row(
            "SELECT id FROM provider_accounts
             WHERE provider_id = ?1 AND execution_scope = 'local' AND is_default = 1",
            params![provider_id],
            |row| row.get(0),
        )
        .optional()
    }

    /// Rename the display label without changing an account ID or credential home.
    ///
    /// @param account_id The profile to rename.
    /// @param label The user-visible name.
    /// @return Success after the label is saved.
    /// @throws rusqlite::Error If SQLite rejects the update.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn rename_provider_account(&self, account_id: &str, label: &str) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .execute(
            "UPDATE provider_accounts SET label = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![label, account_id],
        )?;
        Ok(())
    }

    /// Set one default account for a provider and execution scope.
    ///
    /// @param account The selected profile metadata.
    /// @return Success after replacing the prior default atomically.
    /// @throws rusqlite::Error If SQLite rejects either update.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn set_default_provider_account(&self, account: &ProviderAccount) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let transaction = conn.transaction()?;
        transaction.execute(
            "UPDATE provider_accounts SET is_default = 0
             WHERE provider_id = ?1 AND execution_scope = ?2",
            params![account.provider_id, account.execution_scope],
        )?;
        transaction.execute(
            "UPDATE provider_accounts SET is_default = 1, updated_at = datetime('now')
             WHERE id = ?1",
            params![account.id],
        )?;
        transaction.commit()
    }

    /// Set a project's preferred profile for one provider.
    ///
    /// @param project_id The project whose future sessions use this preference.
    /// @param account The selected compatible account.
    /// @return Success after the preference is saved.
    /// @throws rusqlite::Error If SQLite rejects the upsert.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn set_project_provider_account(
        &self,
        project_id: i64,
        account: &ProviderAccount,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .execute(
                "INSERT INTO project_provider_accounts (project_id, provider_id, account_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id, provider_id) DO UPDATE SET account_id = excluded.account_id",
                params![project_id, account.provider_id, account.id],
            )?;
        Ok(())
    }

    /// Read only a project's explicit account override for one provider.
    ///
    /// @param project_id The project whose override is requested.
    /// @param provider_id The provider for that override.
    /// @return Explicit account ID, or None when provider default applies.
    /// @throws rusqlite::Error If SQLite cannot read the preference.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_project_provider_account(
        &self,
        project_id: i64,
        provider_id: &str,
    ) -> SqlResult<Option<String>> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .query_row(
                "SELECT account_id FROM project_provider_accounts WHERE project_id = ?1 AND provider_id = ?2",
                params![project_id, provider_id],
                |row| row.get(0),
            )
            .optional()
    }

    /// Remove one project override so future sessions use the provider default.
    ///
    /// @param project_id The project whose override is cleared.
    /// @param provider_id The provider whose default should apply again.
    /// @return Success after deleting the override.
    /// @throws rusqlite::Error If SQLite cannot update the preference.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn clear_project_provider_account(
        &self,
        project_id: i64,
        provider_id: &str,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execute(
                "DELETE FROM project_provider_accounts WHERE project_id = ?1 AND provider_id = ?2",
                params![project_id, provider_id],
            )?;
        Ok(())
    }

    /// Store the one explicitly configured account used after an authoritative quota exhaustion.
    ///
    /// @param source_account_id The account that may become exhausted.
    /// @param target_account_id The account selected as its automatic fallback.
    /// @return Success after replacing the previous fallback.
    /// @throws rusqlite::Error If SQLite rejects the fallback mapping.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn set_provider_account_fallback(
        &self,
        source_account_id: &str,
        target_account_id: &str,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execute(
                "INSERT INTO provider_account_fallbacks (source_account_id, target_account_id, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(source_account_id) DO UPDATE SET
                   target_account_id = excluded.target_account_id,
                   updated_at = datetime('now')",
                params![source_account_id, target_account_id],
            )?;
        Ok(())
    }

    /// Read the configured automatic fallback for one account.
    ///
    /// @param source_account_id The account whose fallback is requested.
    /// @return Target account ID, or None when automatic handoff is disabled.
    /// @throws rusqlite::Error If SQLite cannot read the mapping.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn get_provider_account_fallback(
        &self,
        source_account_id: &str,
    ) -> SqlResult<Option<String>> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .query_row(
                "SELECT target_account_id FROM provider_account_fallbacks WHERE source_account_id = ?1",
                params![source_account_id],
                |row| row.get(0),
            )
            .optional()
    }

    /// Disable automatic handoff for one account.
    ///
    /// @param source_account_id The account whose fallback is removed.
    /// @return Success after deleting the mapping.
    /// @throws rusqlite::Error If SQLite cannot delete the mapping.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn clear_provider_account_fallback(&self, source_account_id: &str) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execute(
                "DELETE FROM provider_account_fallbacks WHERE source_account_id = ?1",
                params![source_account_id],
            )?;
        Ok(())
    }

    /// Enable or disable an account in the user's automatic handoff pool.
    ///
    /// @param account_id The account whose checkbox state is being saved.
    /// @param enabled Whether the account may receive an automatic handoff.
    /// @return Success after the preference is persisted.
    /// @throws rusqlite::Error If SQLite rejects the preference.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn set_provider_account_auto_handoff(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> SqlResult<()> {
        self.conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execute(
                "INSERT INTO provider_account_auto_pool (account_id, enabled, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(account_id) DO UPDATE SET
                   enabled = excluded.enabled,
                   updated_at = datetime('now')",
                params![account_id, enabled as i64],
            )?;
        Ok(())
    }

    /// Read whether an account is enabled for automatic quota handoff.
    ///
    /// @param account_id The account whose checkbox state is requested.
    /// @return True when the account is in the automatic handoff pool.
    /// @throws rusqlite::Error If SQLite cannot read the preference.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn provider_account_auto_handoff_enabled(&self, account_id: &str) -> SqlResult<bool> {
        Ok(self
            .conn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .query_row(
                "SELECT enabled FROM provider_account_auto_pool WHERE account_id = ?1",
                params![account_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            != 0)
    }

    /// Select the next configured account that has not already handled this session.
    ///
    /// @param provider_id The provider required by the session.
    /// @param execution_scope The execution scope required by the session.
    /// @param session_id The session whose prior automatic targets are excluded.
    /// @param source_account_id The exhausted account to exclude.
    /// @return The first available account ordered by least recent use.
    /// @throws rusqlite::Error If SQLite cannot read the pool.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn next_auto_handoff_account(
        &self,
        provider_id: &str,
        execution_scope: &str,
        session_id: SessionId,
        source_account_id: &str,
    ) -> SqlResult<Option<ProviderAccount>> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let query = "SELECT a.id, a.provider_id, a.label, a.auth_type, a.status, a.execution_scope,
                    a.profile_home, a.is_default, a.created_at, a.updated_at, a.last_used_at
             FROM provider_accounts a
             JOIN provider_account_auto_pool p ON p.account_id = a.id AND p.enabled = 1
             WHERE a.provider_id = ?1 AND a.execution_scope = ?2 AND a.id != ?3
               AND a.status IN ('available', 'busy', 'near_limit')
               AND NOT EXISTS (
                 SELECT 1 FROM session_account_history h
                 WHERE h.session_id = ?4 AND h.account_id = a.id
                   AND h.event IN ('selected', 'handoff_from', 'handoff_to', 'automatic_handoff_to')
               )
             ORDER BY a.last_used_at IS NOT NULL, a.last_used_at, a.created_at
             LIMIT 1";
        conn.query_row(
            query,
            params![provider_id, execution_scope, source_account_id, session_id],
            provider_account_from_row,
        )
        .optional()
    }

    /// Count active sessions before removing a profile.
    ///
    /// @param account_id The profile whose sessions are counted.
    /// @return The number of running, waiting or initializing sessions.
    /// @throws rusqlite::Error If SQLite cannot count the sessions.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn count_active_account_sessions(&self, account_id: &str) -> SqlResult<i64> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE provider_account_id = ?1
             AND status IN ('running', 'waiting', 'initializing')",
                params![account_id],
                |row| row.get(0),
            )
    }

    /// Disable a profile while retaining all session and usage attribution.
    ///
    /// @param account_id The account being removed from future selection.
    /// @return Success after clearing defaults and marking it unavailable.
    /// @throws rusqlite::Error If SQLite rejects the transaction.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn disable_provider_account(&self, account_id: &str) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let transaction = conn.transaction()?;
        transaction.execute(
            "DELETE FROM project_provider_accounts WHERE account_id = ?1",
            params![account_id],
        )?;
        transaction.execute(
            "DELETE FROM provider_account_fallbacks
             WHERE source_account_id = ?1 OR target_account_id = ?1",
            params![account_id],
        )?;
        transaction.execute(
            "DELETE FROM provider_account_auto_pool WHERE account_id = ?1",
            params![account_id],
        )?;
        transaction.execute(
            "UPDATE provider_accounts SET status = 'unavailable', is_default = 0,
             updated_at = datetime('now') WHERE id = ?1",
            params![account_id],
        )?;
        transaction.commit()
    }

    /// Commit a successful account handoff and its non-sensitive audit history atomically.
    ///
    /// @param session_id The session whose new process has started.
    /// @param previous_account_id The account used before the handoff.
    /// @param next_account_id The account running the new process.
    /// @param model The user-approved model for the next process.
    /// @return Success when binding and history are committed.
    /// @throws rusqlite::Error If SQLite rejects any operation.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn commit_session_account_handoff(
        &self,
        session_id: SessionId,
        previous_account_id: Option<&str>,
        next_account_id: &str,
        model: &str,
    ) -> SqlResult<()> {
        self.commit_session_account_handoff_with_event(
            session_id,
            previous_account_id,
            next_account_id,
            model,
            "handoff_to",
        )
    }

    /// Commit a handoff with an explicit audit event kind.
    ///
    /// @param session_id The session whose new process has started.
    /// @param previous_account_id The account used before the handoff.
    /// @param next_account_id The account running the new process.
    /// @param model The model approved for the next process.
    /// @param target_event The non-sensitive audit event for the target account.
    /// @return Success when binding and history are committed.
    /// @throws rusqlite::Error If SQLite rejects any operation.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    pub fn commit_session_account_handoff_with_event(
        &self,
        session_id: SessionId,
        previous_account_id: Option<&str>,
        next_account_id: &str,
        model: &str,
        target_event: &str,
    ) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let transaction = conn.transaction()?;
        transaction.execute(
            "UPDATE sessions SET provider_account_id = ?1, model = ?2,
             claude_session_id = NULL, updated_at = datetime('now') WHERE id = ?3",
            params![next_account_id, model, session_id],
        )?;
        if let Some(previous_account_id) = previous_account_id {
            transaction.execute(
                "INSERT INTO session_account_history (session_id, account_id, event)
                 VALUES (?1, ?2, 'handoff_from')",
                params![session_id, previous_account_id],
            )?;
        }
        transaction.execute(
            "INSERT INTO session_account_history (session_id, account_id, event)
             VALUES (?1, ?2, ?3)",
            params![session_id, next_account_id, target_event],
        )?;
        transaction.execute(
            "UPDATE provider_accounts SET last_used_at = datetime('now') WHERE id = ?1",
            params![next_account_id],
        )?;
        transaction.commit()
    }

    /// Read a session's account transitions without accessing credentials.
    ///
    /// @param session_id The session whose history is requested.
    /// @return Chronological account transition events.
    /// @throws rusqlite::Error If SQLite cannot read the history.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_session_account_history(
        &self,
        session_id: SessionId,
    ) -> SqlResult<Vec<crate::models::SessionAccountEvent>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement = conn.prepare(
            "SELECT h.account_id, COALESCE(a.label, h.account_id), h.event, h.created_at
             FROM session_account_history h
             LEFT JOIN provider_accounts a ON a.id = h.account_id
             WHERE h.session_id = ?1 ORDER BY h.id",
        )?;
        let history = statement
            .query_map(params![session_id], |row| {
                Ok(crate::models::SessionAccountEvent {
                    account_id: row.get(0)?,
                    label: row.get(1)?,
                    event: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(history)
    }

    /// Recover the original user request from a bounded set of stored output lines.
    ///
    /// @param session_id The Orbit session whose task is needed for local handoff.
    /// @return The earliest stored user message, when present.
    /// @throws rusqlite::Error If SQLite cannot read the stored lines.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    pub fn get_first_session_user_task(&self, session_id: SessionId) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement = conn.prepare(
            "SELECT data FROM session_outputs WHERE session_id = ?1 ORDER BY id ASC LIMIT 100",
        )?;
        let lines = statement
            .query_map(params![session_id], |row| row.get::<_, String>(0))?
            .collect::<SqlResult<Vec<_>>>()?;
        for line in lines {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                if value.get("type").and_then(|item| item.as_str()) == Some("user") {
                    if let Some(task) = value
                        .pointer("/message/content")
                        .and_then(|content| content.as_str())
                    {
                        return Ok(Some(task.to_string()));
                    }
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assistant_text, seed_session, user_text, TestCase};

    fn make_db() -> DatabaseService {
        DatabaseService::open_in_memory().expect("test setup: failed to open in-memory DB")
    }

    /// Verify profile metadata, project defaults and session bindings survive reopening SQLite.
    ///
    /// @return No value; assertions check persistence without storing credentials.
    /// @throws Panic If the migration or account metadata persistence fails.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[test]
    fn should_keep_account_bindings_after_database_restart() {
        let temporary_directory = tempfile::TempDir::new().unwrap();
        let database_path = temporary_directory.path().join("orbit.db");
        let database = DatabaseService::open(&database_path).unwrap();
        let project = database
            .create_project("CRM", "/tmp/crm-restart-test")
            .unwrap();
        let account = ProviderAccount {
            id: "profile-restart".into(),
            provider_id: "codex".into(),
            label: "Work".into(),
            auth_type: AccountAuthType::ChatGPTAuthenticated,
            status: AccountStatus::Available,
            execution_scope: "local".into(),
            profile_home: Some("/tmp/orbit-profile-restart".into()),
            is_default: false,
            created_at: "2026-09-25T00:00:00Z".into(),
            updated_at: "2026-09-25T00:00:00Z".into(),
            last_used_at: None,
        };
        database.create_provider_account(&account).unwrap();
        database
            .set_project_provider_account(project.id, &account)
            .unwrap();
        let session_id = database
            .create_session(
                Some(project.id),
                Some("Agent"),
                "/tmp/crm-restart-test",
                "ignore",
                Some("auto"),
                Some("codex"),
                None,
                None,
            )
            .unwrap();
        database
            .set_session_provider_account(session_id, Some(&account.id))
            .unwrap();
        drop(database);

        let reopened = DatabaseService::open(&database_path).unwrap();
        assert_eq!(
            reopened
                .get_provider_account(&account.id)
                .unwrap()
                .unwrap()
                .label,
            "Work"
        );
        assert_eq!(
            reopened
                .get_project_provider_account(project.id, "codex")
                .unwrap()
                .as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(
            reopened
                .get_session(session_id)
                .unwrap()
                .unwrap()
                .provider_account_id
                .as_deref(),
            Some(account.id.as_str())
        );
    }

    /// Verify separate profile quota and usage attribution survives a session handoff.
    ///
    /// @return No value; assertions verify the persisted account boundaries.
    /// @throws Panic If migration, account binding or history is incorrect.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-25
    #[test]
    fn should_keep_account_quota_and_handoff_history_separate() {
        let database = make_db();
        let project = database
            .create_project("CRM", "/tmp/account-test-crm")
            .unwrap();
        let now = "2026-09-25T00:00:00Z".to_string();
        for (id, label) in [("profile-a", "Personal"), ("profile-b", "Work")] {
            database
                .create_provider_account(&ProviderAccount {
                    id: id.into(),
                    provider_id: "codex".into(),
                    label: label.into(),
                    auth_type: AccountAuthType::ChatGPTAuthenticated,
                    status: AccountStatus::Available,
                    execution_scope: "local".into(),
                    profile_home: Some(format!("/tmp/orbit-test/{id}")),
                    is_default: false,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_used_at: None,
                })
                .unwrap();
        }
        let session_id = database
            .create_session(
                Some(project.id),
                Some("CRM agent"),
                "/tmp/account-test-crm",
                "ignore",
                Some("auto"),
                Some("codex"),
                None,
                None,
            )
            .unwrap();
        database
            .set_session_provider_account(session_id, Some("profile-a"))
            .unwrap();
        for (account_id, utilization) in [("profile-a", 0.72), ("profile-b", 0.19)] {
            database
                .record_provider_quota(&crate::models::ProviderQuota {
                    provider: "codex".into(),
                    account_key: account_id.into(),
                    provider_account_id: Some(account_id.into()),
                    five_hour: Some(crate::models::QuotaWindow {
                        utilization,
                        resets_at: None,
                        status: Some("normal".into()),
                    }),
                    seven_day: None,
                    updated_at: now.clone(),
                    source: "cli_event".into(),
                })
                .unwrap();
        }
        let quotas = database.get_latest_provider_quotas().unwrap();
        assert_eq!(quotas.len(), 2);
        assert!(quotas.iter().any(|quota| {
            quota.provider_account_id.as_deref() == Some("profile-a")
                && quota.five_hour.as_ref().unwrap().utilization == 0.72
        }));
        database
            .commit_session_account_handoff(session_id, Some("profile-a"), "profile-b", "auto")
            .unwrap();
        assert_eq!(
            database
                .get_session(session_id)
                .unwrap()
                .unwrap()
                .provider_account_id
                .as_deref(),
            Some("profile-b")
        );
        let history = database.get_session_account_history(session_id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].account_id, "profile-a");
        assert_eq!(history[0].event, "selected");
        assert_eq!(history[1].account_id, "profile-a");
        assert_eq!(history[2].account_id, "profile-b");
    }

    /// Verify the usage dashboard can read quota snapshots without re-locking SQLite.
    ///
    /// @return No value; assertions verify the overview query completes successfully.
    /// @throws Panic If the usage overview cannot be assembled from an empty database.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    #[test]
    fn should_build_usage_overview_without_reentrant_connection_lock() {
        let database = make_db();
        let overview = database
            .get_usage_overview()
            .expect("usage overview should not deadlock while reading quotas");

        assert_eq!(overview.total_tokens_today, 0);
        assert!(overview.quotas.is_empty());
    }

    /// Verify checked profiles are selected one time each without returning to an exhausted account.
    ///
    /// @return No value; assertions verify pool selection and session history guards.
    /// @throws Panic If the automatic pool does not respect account history.
    /// @author ductv <ductv@getflycrm.com>
    /// @since 2026-09-26
    #[test]
    fn should_select_each_enabled_auto_handoff_account_once() {
        let database = make_db();
        let project = database
            .create_project("CRM", "/tmp/account-auto-pool-test")
            .unwrap();
        let now = "2026-09-26T00:00:00Z".to_string();
        for (id, label) in [
            ("profile-a", "Personal"),
            ("profile-b", "Work"),
            ("profile-c", "API"),
        ] {
            database
                .create_provider_account(&ProviderAccount {
                    id: id.into(),
                    provider_id: "codex".into(),
                    label: label.into(),
                    auth_type: AccountAuthType::ChatGPTAuthenticated,
                    status: AccountStatus::Available,
                    execution_scope: "local".into(),
                    profile_home: Some(format!("/tmp/orbit-test/{id}")),
                    is_default: false,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_used_at: None,
                })
                .unwrap();
            database
                .set_provider_account_auto_handoff(id, true)
                .unwrap();
        }
        let session_id = database
            .create_session(
                Some(project.id),
                Some("CRM agent"),
                "/tmp/account-auto-pool-test",
                "ignore",
                Some("auto"),
                Some("codex"),
                None,
                None,
            )
            .unwrap();
        database
            .set_session_provider_account(session_id, Some("profile-a"))
            .unwrap();

        let next = database
            .next_auto_handoff_account("codex", "local", session_id, "profile-a")
            .unwrap()
            .unwrap();
        assert_eq!(next.id, "profile-b");
        database
            .commit_session_account_handoff_with_event(
                session_id,
                Some("profile-a"),
                "profile-b",
                "auto",
                "automatic_handoff_to",
            )
            .unwrap();
        let next = database
            .next_auto_handoff_account("codex", "local", session_id, "profile-b")
            .unwrap()
            .unwrap();
        assert_eq!(next.id, "profile-c");
    }

    // ── Projects ─────────────────────────────────────────────────────────

    #[test]
    fn should_create_tables_on_migrate() {
        let mut t = TestCase::new("should_create_tables_on_migrate");
        t.phase("Act");
        let db = make_db();
        t.phase("Assert");
        let sessions = db.get_sessions().expect("get_sessions failed");
        t.empty("sessions table exists and is empty", &sessions);
    }

    #[test]
    fn should_create_project_with_correct_fields() {
        let mut t = TestCase::new("should_create_project_with_correct_fields");
        t.phase("Act");
        let db = make_db();
        let p = db
            .create_project("my-app", "/home/user/my-app")
            .expect("create_project failed");
        t.phase("Assert");
        t.eq("name matches", p.name.as_str(), "my-app");
        t.eq("path matches", p.path.as_str(), "/home/user/my-app");
        t.ok("id is positive", p.id > 0);
    }

    #[test]
    fn should_return_same_project_when_path_already_exists() {
        let mut t = TestCase::new("should_return_same_project_when_path_already_exists");
        t.phase("Seed");
        let db = make_db();
        let first = db
            .create_project("my-app", "/home/user/my-app")
            .expect("first failed");
        t.phase("Act");
        let second = db
            .create_project("my-app", "/home/user/my-app")
            .expect("second failed");
        t.phase("Assert");
        t.eq("same ID (idempotent)", first.id, second.id);
    }

    #[test]
    fn should_list_all_projects_ordered_by_name() {
        let mut t = TestCase::new("should_list_all_projects_ordered_by_name");
        t.phase("Seed");
        let db = make_db();
        db.create_project("beta", "/beta").expect("seed beta");
        db.create_project("alpha", "/alpha").expect("seed alpha");
        t.phase("Act");
        let projects = db.get_projects().expect("get_projects failed");
        t.phase("Assert");
        t.len("two projects", &projects, 2);
        t.eq("first is alpha (ASC)", projects[0].name.as_str(), "alpha");
    }

    // ── Sessions ─────────────────────────────────────────────────────────

    #[test]
    fn should_create_session_with_initializing_status() {
        let mut t = TestCase::new("should_create_session_with_initializing_status");
        t.phase("Act");
        let db = make_db();
        let id = db
            .create_session(
                None,
                Some("task 1"),
                "/tmp/proj",
                "ignore",
                None,
                None,
                None,
                None,
            )
            .expect("create_session failed");
        t.phase("Assert");
        t.ok("id is positive", id > 0);
        let sessions = db.get_sessions().expect("get_sessions failed");
        t.len("one session", &sessions, 1);
        t.eq(
            "status is initializing",
            &sessions[0].status,
            &crate::models::SessionStatus::Initializing,
        );
    }

    #[test]
    fn should_store_cwd_on_session_create() {
        let mut t = TestCase::new("should_store_cwd_on_session_create");
        t.phase("Act");
        let db = make_db();
        let id = db
            .create_session(None, None, "/tmp/proj", "ignore", None, None, None, None)
            .expect("create failed");
        t.phase("Assert");
        let session = db
            .get_session(id)
            .expect("get failed")
            .expect("session missing");
        t.eq("cwd stored", session.cwd.as_deref(), Some("/tmp/proj"));
    }

    #[test]
    fn should_update_session_status() {
        let mut t = TestCase::new("should_update_session_status");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        t.phase("Act");
        db.update_session_status(id, crate::models::SessionStatus::Running)
            .expect("update failed");
        t.phase("Assert");
        let sessions = db.get_sessions().expect("get_sessions failed");
        t.eq(
            "status updated to running",
            &sessions[0].status,
            &crate::models::SessionStatus::Running,
        );
    }

    #[test]
    fn should_set_running_status_and_pid_on_update_pid() {
        let mut t = TestCase::new("should_set_running_status_and_pid_on_update_pid");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        t.phase("Act");
        db.update_session_pid(id, 12345).expect("update_pid failed");
        t.phase("Assert");
        let s = db.get_session(id).expect("get failed").expect("missing");
        t.eq(
            "status is running",
            &s.status,
            &crate::models::SessionStatus::Running,
        );
        t.eq("pid stored", s.pid, Some(12345));
    }

    #[test]
    fn should_return_none_for_missing_session_id() {
        let mut t = TestCase::new("should_return_none_for_missing_session_id");
        t.phase("Act");
        let db = make_db();
        let result = db.get_session(999).expect("get_session failed");
        t.phase("Assert");
        t.none("returns None for unknown id", &result);
    }

    #[test]
    fn should_associate_session_with_project_via_foreign_key() {
        let mut t = TestCase::new("should_associate_session_with_project_via_foreign_key");
        t.phase("Seed");
        let db = make_db();
        let project = db.create_project("myapp", "/myapp").expect("seed project");
        t.phase("Act");
        let id = db
            .create_session(
                Some(project.id),
                Some("feat"),
                "/myapp",
                "approve",
                Some("claude-sonnet-4-6"),
                None,
                None,
                None,
            )
            .expect("create failed");
        t.phase("Assert");
        let s = db.get_session(id).expect("get failed").expect("missing");
        t.eq("project_id stored", s.project_id, Some(project.id));
        t.eq(
            "model stored",
            s.model.as_deref(),
            Some("claude-sonnet-4-6"),
        );
    }

    #[test]
    fn should_rename_session() {
        let mut t = TestCase::new("should_rename_session");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        t.phase("Act");
        db.rename_session(id, "new-name").expect("rename failed");
        t.phase("Assert");
        let s = db.get_session(id).expect("get failed").expect("missing");
        t.eq("name updated", s.name.as_deref(), Some("new-name"));
    }

    #[test]
    fn should_store_and_retrieve_worktree_path() {
        let mut t = TestCase::new("should_store_and_retrieve_worktree_path");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        t.phase("Act");
        db.update_session_worktree(id, "/tmp/wt/branch", "orbit/my-branch")
            .expect("update_worktree failed");
        t.phase("Assert");
        let s = db.get_session(id).expect("get failed").expect("missing");
        t.eq(
            "worktree_path stored",
            s.worktree_path.as_deref(),
            Some("/tmp/wt/branch"),
        );
        t.eq(
            "branch_name stored",
            s.branch_name.as_deref(),
            Some("orbit/my-branch"),
        );
    }

    #[test]
    fn should_persist_and_retrieve_claude_session_id() {
        let mut t = TestCase::new("should_persist_and_retrieve_claude_session_id");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        t.phase("Act");
        db.update_claude_session_id(id, "claude-abc-123")
            .expect("update failed");
        let result = db.get_claude_session_id(id).expect("get failed");
        t.phase("Assert");
        t.eq(
            "claude_session_id stored",
            result.as_deref(),
            Some("claude-abc-123"),
        );
    }

    // ── Outputs ──────────────────────────────────────────────────────────

    #[test]
    fn should_insert_and_retrieve_outputs_in_order() {
        let mut t = TestCase::new("should_insert_and_retrieve_outputs_in_order");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        let line1 = assistant_text("first message");
        let line2 = user_text("second message");
        db.insert_output(id, &line1).expect("insert 1");
        db.insert_output(id, &line2).expect("insert 2");
        db.flush_outputs();
        t.phase("Act");
        let rows = db.get_outputs(id).expect("get_outputs failed");
        t.phase("Assert");
        t.len("two rows", &rows, 2);
        t.eq("first row matches", rows[0].as_str(), line1.as_str());
    }

    #[test]
    fn should_isolate_outputs_per_session() {
        let mut t = TestCase::new("should_isolate_outputs_per_session");
        t.phase("Seed");
        let db = make_db();
        let id1 = db
            .create_session(None, None, "/a", "ignore", None, None, None, None)
            .expect("s1");
        let id2 = db
            .create_session(None, None, "/b", "ignore", None, None, None, None)
            .expect("s2");
        db.insert_output(id1, &assistant_text("session 1 msg"))
            .expect("o1");
        db.insert_output(id2, &assistant_text("session 2 msg"))
            .expect("o2");
        db.flush_outputs();
        t.phase("Assert");
        t.len("session 1 has 1 output", &db.get_outputs(id1).unwrap(), 1);
        t.len("session 2 has 1 output", &db.get_outputs(id2).unwrap(), 1);
    }

    // ── Delete (atomicity) ────────────────────────────────────────────────

    #[test]
    fn should_delete_session_and_its_outputs_together() {
        let mut t = TestCase::new("should_delete_session_and_its_outputs_together");
        t.phase("Seed");
        let db = make_db();
        let id = seed_session(&db);
        db.insert_output(id, &assistant_text("first"))
            .expect("insert");
        db.insert_output(id, &user_text("second"))
            .expect("insert 2");
        db.flush_outputs();
        t.phase("Act");
        db.delete_session(id).expect("delete failed");
        t.phase("Assert");
        t.none("session row removed", &db.get_session(id).expect("get"));
        t.empty("outputs removed", &db.get_outputs(id).expect("outputs"));
    }

    #[test]
    fn should_round_trip_session_status_as_enum() {
        let mut t = TestCase::new("should_round_trip_session_status_as_enum");
        t.phase("Seed");
        let db = make_db();
        let sid = db
            .create_session(None, None, "/tmp", "ignore", None, None, None, None)
            .expect("session");
        db.update_session_status(sid, crate::models::SessionStatus::Stopped)
            .expect("update");
        t.phase("Act");
        let sessions = db.get_sessions().expect("get");
        t.phase("Assert");
        t.eq(
            "status is SessionStatus::Stopped",
            &sessions[0].status,
            &crate::models::SessionStatus::Stopped,
        );
    }

    #[test]
    fn should_store_and_retrieve_ssh_host_and_user() {
        let mut t = TestCase::new("should_store_and_retrieve_ssh_host_and_user");

        t.phase("Seed");
        let db = DatabaseService::open_in_memory().unwrap();
        let pid = db
            .create_session(
                None,
                Some("ssh-test"),
                "/tmp",
                "ignore",
                Some("claude-sonnet"),
                Some("claude-code"),
                Some("vps.example.com"),
                Some("ubuntu"),
            )
            .unwrap();

        t.phase("Assert SSH session");
        let s = db.get_session(pid).unwrap().unwrap();
        t.eq(
            "ssh_host stored",
            s.ssh_host.as_deref(),
            Some("vps.example.com"),
        );
        t.eq("ssh_user stored", s.ssh_user.as_deref(), Some("ubuntu"));

        t.phase("Assert local session has null SSH");
        let pid2 = db
            .create_session(
                None,
                Some("local-test"),
                "/tmp",
                "ignore",
                Some("claude-sonnet"),
                Some("claude-code"),
                None,
                None,
            )
            .unwrap();
        let s2 = db.get_session(pid2).unwrap().unwrap();
        t.none("ssh_host is None for local session", &s2.ssh_host);
        t.none("ssh_user is None for local session", &s2.ssh_user);
    }

    // ── Pipeline DB tests ───────────────────────────────────────────────────

    fn default_pipeline_config() -> crate::pipeline::models::PipelineConfig {
        crate::pipeline::models::PipelineConfig::default_preset("claude-code", "auto")
    }

    #[test]
    fn should_create_and_retrieve_pipeline() {
        let mut t = TestCase::new("should_create_and_retrieve_pipeline");
        let db = DatabaseService::open_in_memory().unwrap();

        t.phase("Create");
        let cfg = default_pipeline_config();
        let id = db
            .create_pipeline("PayPal feature", "Add PayPal payment", Some("/repo"), &cfg)
            .expect("create failed");
        t.ok("create returns positive id", id > 0);

        t.phase("Retrieve");
        let p = db.get_pipeline(id).expect("get failed").expect("missing");
        t.eq("name stored", p.name.as_str(), "PayPal feature");
        t.eq(
            "user_request stored",
            p.user_request.as_str(),
            "Add PayPal payment",
        );
        t.eq(
            "worktree_path stored",
            p.worktree_path.as_deref(),
            Some("/repo"),
        );
        t.eq(
            "status is created",
            &p.status,
            &crate::pipeline::models::PipelineStatus::Created,
        );
        t.eq("no steps yet", p.steps.len(), 0);
    }

    #[test]
    fn should_update_pipeline_status() {
        let mut t = TestCase::new("should_update_pipeline_status");
        let db = DatabaseService::open_in_memory().unwrap();
        let id = db
            .create_pipeline("test", "req", None, &default_pipeline_config())
            .unwrap();

        t.phase("Transition to Planning");
        db.update_pipeline_status(id, &crate::pipeline::models::PipelineStatus::Planning)
            .expect("update failed");
        let p = db.get_pipeline(id).unwrap().unwrap();
        t.eq(
            "status is planning",
            &p.status,
            &crate::pipeline::models::PipelineStatus::Planning,
        );
    }

    #[test]
    fn should_create_and_list_pipeline_steps() {
        let mut t = TestCase::new("should_create_and_list_pipeline_steps");
        let db = DatabaseService::open_in_memory().unwrap();
        let pid = db
            .create_pipeline("pipeline", "req", None, &default_pipeline_config())
            .unwrap();

        t.phase("Add steps");
        db.create_pipeline_step(
            pid,
            &crate::pipeline::models::PipelineAgentRole::Planner,
            "claude-code",
            Some("opus"),
        )
        .expect("planner step failed");
        db.create_pipeline_step(
            pid,
            &crate::pipeline::models::PipelineAgentRole::Developer,
            "codex",
            Some("gpt-latest"),
        )
        .expect("developer step failed");

        t.phase("Retrieve");
        let p = db.get_pipeline(pid).unwrap().unwrap();
        t.eq("2 steps", p.steps.len(), 2);
        t.eq(
            "first role is planner",
            &p.steps[0].role,
            &crate::pipeline::models::PipelineAgentRole::Planner,
        );
        t.eq(
            "second role is developer",
            &p.steps[1].role,
            &crate::pipeline::models::PipelineAgentRole::Developer,
        );
        t.eq(
            "first step is pending",
            &p.steps[0].status,
            &crate::pipeline::models::PipelineStepStatus::Pending,
        );
    }

    #[test]
    fn should_list_pipelines_newest_first() {
        let mut t = TestCase::new("should_list_pipelines_newest_first");
        let db = DatabaseService::open_in_memory().unwrap();
        let cfg = default_pipeline_config();
        db.create_pipeline("first", "r", None, &cfg).unwrap();
        db.create_pipeline("second", "r", None, &cfg).unwrap();
        db.create_pipeline("third", "r", None, &cfg).unwrap();

        let list = db.list_pipelines().unwrap();
        t.eq("3 pipelines", list.len(), 3);
        t.eq("newest first", list[0].name.as_str(), "third");
        t.eq("oldest last", list[2].name.as_str(), "first");
    }

    #[test]
    fn should_save_and_version_pipeline_artifact() {
        let mut t = TestCase::new("should_save_and_version_pipeline_artifact");
        let db = DatabaseService::open_in_memory().unwrap();
        let pid = db
            .create_pipeline("p", "r", None, &default_pipeline_config())
            .unwrap();

        let v1 = serde_json::json!({"plan": "v1"});
        let v2 = serde_json::json!({"plan": "v2"});

        db.save_pipeline_artifact(pid, None, "implementation_plan", &v1)
            .expect("v1 failed");
        db.save_pipeline_artifact(pid, None, "implementation_plan", &v2)
            .expect("v2 failed");

        // Verify both versions exist with correct version numbers
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pipeline_artifacts WHERE pipeline_id = ?1 AND artifact_type = 'implementation_plan'",
                rusqlite::params![pid],
                |r| r.get(0),
            )
            .unwrap();
        t.eq("2 artifact versions stored", count, 2);
    }

    #[test]
    fn should_append_pipeline_events() {
        let mut t = TestCase::new("should_append_pipeline_events");
        let db = DatabaseService::open_in_memory().unwrap();
        let pid = db
            .create_pipeline("p", "r", None, &default_pipeline_config())
            .unwrap();

        db.append_pipeline_event(pid, None, "pipeline_created", None)
            .expect("event 1 failed");
        db.append_pipeline_event(
            pid,
            None,
            "status_changed_planning",
            Some(&serde_json::json!({"from": "created", "to": "planning"})),
        )
        .expect("event 2 failed");

        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pipeline_events WHERE pipeline_id = ?1",
                rusqlite::params![pid],
                |r| r.get(0),
            )
            .unwrap();
        t.eq("2 events stored", count, 2);
    }
}
