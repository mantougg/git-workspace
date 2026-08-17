//! T-24 Task DAG + T-23 Workspace Pipeline commands.
//!
//! DAG commands expose the scheduling kernel (submit / visualize / cancel);
//! pipeline commands compile a pipeline definition into a DAG run and serve
//! templates (JSON file in the app data dir) and the execution report.

use tauri::State;

use crate::core::pipeline::{self, Pipeline, PipelineRunReport, RepoSelection};
use crate::error::{AppError, AppResult};
use crate::models::task::{DagGraph, DagSubmitRequest, FailurePolicy};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// T-24 Task DAG
// ---------------------------------------------------------------------------

/// Submit a dependency DAG of tasks (T-24). `dependsOn` entries are indices
/// into `request.nodes`. Returns the DAG id (also the nodes' `batchId`).
#[tauri::command]
pub fn submit_dag_tasks(
    request: DagSubmitRequest,
    state: State<'_, AppState>,
) -> AppResult<String> {
    state.task_manager.submit_dag(&request)
}

/// DAG visualization payload: nodes + edges + live states (T-24).
#[tauri::command]
pub fn get_dag_graph(dag_id: String, state: State<'_, AppState>) -> AppResult<DagGraph> {
    state
        .task_manager
        .get_dag_graph(&dag_id)
        .ok_or_else(|| AppError::NotFound(format!("DAG {} 不存在或已过期", dag_id)))
}

/// Cancel a whole DAG run: pending nodes are marked cancelled (propagated
/// along dependency edges), running nodes get the cooperative cancel flag.
#[tauri::command]
pub fn cancel_dag(dag_id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.task_manager.cancel_dag(&dag_id)
}

// ---------------------------------------------------------------------------
// T-23 Workspace Pipeline
// ---------------------------------------------------------------------------

/// List all saved pipeline templates.
#[tauri::command]
pub fn list_pipeline_templates() -> AppResult<Vec<Pipeline>> {
    Ok(pipeline::load_templates())
}

/// Save (upsert) a pipeline template. New templates (empty id) get an id and
/// timestamps assigned; the saved template is returned.
#[tauri::command]
pub fn save_pipeline_template(mut template: Pipeline) -> AppResult<Pipeline> {
    pipeline::validate_pipeline(&template).map_err(AppError::Task)?;

    let mut all = pipeline::load_templates();
    let now = chrono::Utc::now().to_rfc3339();
    if template.id.is_empty() {
        template.id = uuid::Uuid::new_v4().to_string();
        template.created_at = now.clone();
    }
    template.updated_at = now;
    if let Some(existing) = all.iter_mut().find(|p| p.id == template.id) {
        template.created_at = existing.created_at.clone();
        *existing = template.clone();
    } else {
        all.push(template.clone());
    }
    pipeline::save_templates(&all)?;
    Ok(template)
}

/// Delete a pipeline template by id.
#[tauri::command]
pub fn delete_pipeline_template(template_id: String) -> AppResult<()> {
    let mut all = pipeline::load_templates();
    all.retain(|p| p.id != template_id);
    pipeline::save_templates(&all)
}

/// The built-in sample flow (Fetch All → Check Status → Pull Clean → Build
/// → Test → Report), returned unsaved — persist via `save_pipeline_template`.
#[tauri::command]
pub fn get_sample_pipeline() -> AppResult<Pipeline> {
    Ok(pipeline::sample_pipeline())
}

/// Compile a pipeline over the selected repositories into a DAG and submit
/// it (T-23 on T-24). Returns the run id (= DAG id).
#[tauri::command]
pub fn run_pipeline(
    pipeline: Pipeline,
    repos: Vec<RepoSelection>,
    on_failure: Option<FailurePolicy>,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let request = pipeline::compile_pipeline(
        &pipeline,
        &repos,
        on_failure.unwrap_or_default(),
    )
    .map_err(AppError::Task)?;
    state.task_manager.submit_dag(&request)
}

/// Execution report of a pipeline run (T-23): per-step aggregates + per-repo
/// results, durations, partial-failure detail. The pipeline definition is
/// passed back by the caller (the UI holds it); live states come from the
/// in-memory DAG.
#[tauri::command]
pub fn get_pipeline_run(
    run_id: String,
    pipeline: Pipeline,
    state: State<'_, AppState>,
) -> AppResult<PipelineRunReport> {
    state
        .task_manager
        .with_dag(&run_id, |dag| pipeline::build_run_report(&run_id, &pipeline, dag))
        .ok_or_else(|| AppError::NotFound(format!("运行 {} 不存在或已过期", run_id)))
}
