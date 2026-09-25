use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, State};

use crate::ipc::IpcError;
use crate::pipeline::models::{
    CreatePipelineRequest, Pipeline, PipelineConfig, PipelineId, PipelineStatus,
};
use crate::pipeline::PipelineOrchestrator;
use crate::providers::ProviderRegistry;
use crate::services::database::DatabaseService;
use crate::services::session_manager::SessionManager;

pub struct PipelineState(pub Arc<Mutex<PipelineOrchestrator>>);

impl PipelineState {
    pub fn new(
        db: Arc<DatabaseService>,
        session_manager: Arc<RwLock<SessionManager>>,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self(Arc::new(Mutex::new(PipelineOrchestrator::new(
            db,
            session_manager,
            registry,
        ))))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PipelineOrchestrator> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[tauri::command]
pub fn create_pipeline(
    name: String,
    user_request: String,
    worktree_path: String,
    config: PipelineConfig,
    state: State<PipelineState>,
    app: AppHandle,
) -> Result<Pipeline, IpcError> {
    let pipeline = {
        let orch = state.lock();
        let p = orch
            .create(CreatePipelineRequest {
                name,
                user_request,
                worktree_path,
                config,
            })
            .map_err(IpcError::Other)?;
        orch.start(p.id, app.clone()).map_err(IpcError::Other)?;
        p
    };
    use tauri::Emitter;
    let _ = app.emit("pipeline:created", &pipeline);
    Ok(pipeline)
}

#[tauri::command]
pub fn list_pipelines(state: State<PipelineState>) -> Result<Vec<Pipeline>, IpcError> {
    state
        .lock()
        .db
        .list_pipelines()
        .map_err(|e| IpcError::Other(e.to_string()))
}

#[tauri::command]
pub fn get_pipeline(
    pipeline_id: PipelineId,
    state: State<PipelineState>,
) -> Result<Option<Pipeline>, IpcError> {
    state
        .lock()
        .db
        .get_pipeline(pipeline_id)
        .map_err(|e| IpcError::Other(e.to_string()))
}

#[tauri::command]
pub fn cancel_pipeline(
    pipeline_id: PipelineId,
    state: State<PipelineState>,
    app: AppHandle,
) -> Result<Pipeline, IpcError> {
    state
        .lock()
        .cancel(pipeline_id, &app)
        .map_err(IpcError::Other)
}

/// Transition a pipeline to a new status (used internally and for dev tooling).
#[tauri::command]
pub fn set_pipeline_status(
    pipeline_id: PipelineId,
    status: String,
    state: State<PipelineState>,
    app: AppHandle,
) -> Result<Pipeline, IpcError> {
    let to: PipelineStatus = status.parse().map_err(|e: String| IpcError::Other(e))?;
    state
        .lock()
        .transition(pipeline_id, to, &app)
        .map_err(IpcError::Other)
}
