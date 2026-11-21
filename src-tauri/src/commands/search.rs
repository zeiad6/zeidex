use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;
use zeidex_engine::LaunchResult;

use crate::state::AppState;

/// Launcher search. `limit` is optional so the UI can omit it for the default.
#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<LaunchResult>, String> {
    let engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(engine.search(&query, limit.unwrap_or(10)))
}

/// Walks apps/files/settings and rebuilds the candidate table. Runs on the
/// blocking pool so the UI thread never stalls on the filesystem walk.
#[tauri::command]
pub async fn bootstrap_index(state: State<'_, AppState>) -> Result<usize, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        zeidex_engine::QueryEngine::bootstrap_sqlite(&db_path).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())??;

    state.reload_engine();
    let engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(engine.candidate_count())
}

#[tauri::command]
pub fn candidate_count(state: State<'_, AppState>) -> Result<usize, String> {
    let engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(engine.candidate_count())
}

/// Records a launch in SQLite and bumps the in-memory score, so the next search
/// ranks the used candidate higher without waiting for a re-index.
#[tauri::command]
pub fn record_usage(
    state: State<'_, AppState>,
    candidate_id: String,
    action: String,
) -> Result<(), String> {
    {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store
            .record_usage_event(&candidate_id, &action)
            .map_err(|err| err.to_string())?;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let mut engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    engine.record_usage_in_memory(&candidate_id, now);
    Ok(())
}
