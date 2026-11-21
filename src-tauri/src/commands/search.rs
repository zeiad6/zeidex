use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;
use zeidex_engine::{LaunchResult, LaunchResultAction};
use zeidex_storage::SearchSettings;

use crate::state::AppState;

/// Launcher search. `limit` is optional so the UI can omit it for the default.
///
/// `(async)` is not about awaiting anything: without it a command runs on the
/// main thread, so the database lookup and the serialization of every result
/// would sit between two keystrokes on the UI thread. The lock is taken and
/// released inside this call, so nothing is held across a suspension point.
#[tauri::command(async)]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<LaunchResult>, String> {
    let limit = limit.unwrap_or(10);
    let mut results = {
        let engine = state
            .engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        engine.search(&query, limit)
    };
    // The web row is composed here, not in the engine: it depends on the stored
    // settings and matches no indexed candidate. It is appended last so it can
    // never outrank a real local match.
    let settings = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store
            .load_search_settings()
            .map_err(|err| err.to_string())?
    };
    if let Some(web) = web_search_result(&query, settings) {
        results.push(web);
    }
    Ok(results)
}

/// The "search the web for …" row, or `None` when the user turned web search off
/// or typed nothing. Pure so the shape and the truncation are tested without a
/// Tauri app.
fn web_search_result(query: &str, settings: SearchSettings) -> Option<LaunchResult> {
    let trimmed = query.trim();
    if !settings.web_search_enabled || trimmed.is_empty() {
        return None;
    }

    // A pasted paragraph is a search query too, but sixty characters is all a
    // one-line result can show; the action carries the full text.
    const MAX_TITLE_CHARS: usize = 60;
    let mut title: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    if trimmed.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }

    Some(LaunchResult {
        // Not a candidate id: there is no row for `record_usage` to update, and
        // the launcher skips usage recording for this action.
        id: "shell:web-search".to_string(),
        kind: "web".to_string(),
        title,
        subtitle: Some(format!("Search {}", settings.web_search_engine.label())),
        path: String::new(),
        // Below every local score, so anything that re-sorts honours the
        // appended position.
        score: i64::MIN / 2,
        action: LaunchResultAction::WebSearch {
            query: trimmed.to_string(),
        },
        icon: None,
    })
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

#[tauri::command(async)]
pub fn candidate_count(state: State<'_, AppState>) -> Result<usize, String> {
    let engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(engine.candidate_count())
}

/// Records a launch in SQLite and bumps the in-memory score, so the next search
/// ranks the used candidate higher without waiting for a re-index.
#[tauri::command(async)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use zeidex_storage::SearchEngine;

    fn settings(enabled: bool, engine: SearchEngine) -> SearchSettings {
        SearchSettings {
            web_search_enabled: enabled,
            web_search_engine: engine,
        }
    }

    #[test]
    fn web_row_is_absent_when_disabled_or_the_query_is_blank() {
        assert!(
            web_search_result("rust", settings(false, SearchEngine::Google)).is_none(),
            "a disabled setting must remove the row, not just ignore its click"
        );
        assert!(web_search_result("   ", settings(true, SearchEngine::Google)).is_none());
    }

    #[test]
    fn web_row_carries_the_full_query_and_the_chosen_engine() {
        let row = web_search_result(
            "  مرحبا بالعالم  ",
            settings(true, SearchEngine::DuckDuckGo),
        )
        .expect("row");
        assert_eq!(row.kind, "web");
        assert_eq!(row.title, "مرحبا بالعالم");
        assert_eq!(row.subtitle.as_deref(), Some("Search DuckDuckGo"));
        match row.action {
            LaunchResultAction::WebSearch { query } => assert_eq!(query, "مرحبا بالعالم"),
            other => panic!("expected a web action, got {other:?}"),
        }
    }

    #[test]
    fn web_row_truncates_only_the_title() {
        let query = "a".repeat(100);
        let row = web_search_result(&query, settings(true, SearchEngine::Bing)).expect("row");
        assert_eq!(row.title.chars().count(), 61, "60 chars plus the ellipsis");
        assert!(row.title.ends_with('…'));
        match row.action {
            LaunchResultAction::WebSearch { query: carried } => {
                assert_eq!(
                    carried.chars().count(),
                    100,
                    "the action keeps the whole query"
                );
            }
            other => panic!("expected a web action, got {other:?}"),
        }
    }
}
