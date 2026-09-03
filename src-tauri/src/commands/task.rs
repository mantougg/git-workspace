use tauri::State;

use crate::error::AppResult;
use crate::models::task::Task;
use crate::state::AppState;

/// Submit a batch of tasks to the background queue.
/// Returns the list of generated task IDs.
#[tauri::command]
pub fn submit_tasks(
    tasks: Vec<crate::models::task::TaskRequest>,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    state.task_manager.submit(&tasks)
}

/// Query the status of multiple tasks by ID.
#[tauri::command]
pub fn get_task_status(task_ids: Vec<String>, state: State<'_, AppState>) -> AppResult<Vec<Task>> {
    Ok(state.task_manager.get_status(&task_ids))
}

/// Cancel a queued task. Running tasks cannot be cancelled.
#[tauri::command]
pub fn cancel_task(task_id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.task_manager.cancel(&task_id)
}

/// List all active tasks (queued + running + recently finished).
/// Finished tasks are auto-removed after 30 seconds.
#[tauri::command]
pub fn list_active_tasks(state: State<'_, AppState>) -> AppResult<Vec<Task>> {
    Ok(state.task_manager.list_active())
}

/// Clear finished tasks from the active list.
#[tauri::command]
pub fn clear_finished_tasks(state: State<'_, AppState>) -> AppResult<()> {
    state.task_manager.cleanup_finished();
    Ok(())
}
