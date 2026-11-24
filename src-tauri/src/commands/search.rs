use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};
use zeidex_engine::{LaunchResult, LaunchResultAction, QueryEngine};
use zeidex_storage::{DocumentHit, SearchSettings};

use crate::index_service::IndexService;
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
    // Text the Studio recognized, indexed locally: a file is findable by a
    // phrase inside it. A database error here must not break the launcher — the
    // name results above are already correct — so it is reported to stderr and
    // the search returns what it has. The same applies to the settings read
    // below, which is why it stays a hard error only for the web row.
    let document_hits = if query.trim().chars().count() >= MIN_CONTENT_QUERY_CHARS {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match store.search_document_text(&query, MAX_CONTENT_HITS) {
            Ok(hits) => hits,
            Err(err) => {
                eprintln!("zeidex search: document text lookup failed: {err}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    merge_document_hits(&mut results, document_hits);
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

/// How many content hits one query may add. Two keeps the panel about files
/// first while still surfacing the scan the user was actually looking for.
const MAX_CONTENT_HITS: usize = 2;

/// Shortest query that searches document text. A single letter would match
/// nearly every stored page and bury the name matches the user typed.
const MIN_CONTENT_QUERY_CHARS: usize = 3;

/// Append content hits for files the name search did not already return.
///
/// A content hit is a real file, so it opens like any other result. Its id is
/// prefixed `doc:` because it has no candidate row: `record_usage` would find
/// nothing to bump, which is harmless and keeps the launcher's usage table
/// about names the user actually launched.
fn merge_document_hits(results: &mut Vec<LaunchResult>, hits: Vec<DocumentHit>) {
    if hits.is_empty() {
        return;
    }
    let known: std::collections::HashSet<String> =
        results.iter().map(|result| result.path.clone()).collect();
    for hit in hits {
        if known.contains(&hit.path) {
            continue;
        }
        let title = hit
            .path
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&hit.path)
            .to_string();
        results.push(LaunchResult {
            id: format!("doc:{}", hit.path),
            kind: "document".to_string(),
            title,
            subtitle: Some(hit.snippet),
            path: hit.path.clone(),
            // Below every scored candidate: a name match outranks a content
            // match, and a content match still outranks the web row.
            score: 0,
            action: LaunchResultAction::Open {
                path: hit.path.clone(),
            },
            icon: None,
            // A content hit matched the text inside the file, not its name.
            match_kind: None,
            // A document hit comes from the content index, which carries no
            // filesystem facts; the card falls back to path and match type.
            size_bytes: None,
            modified_unix_s: None,
        });
    }
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
        // The web row is not a name match against an indexed candidate.
        match_kind: None,
        // Nothing on disk: a web row has no size and no modification time.
        size_bytes: None,
        modified_unix_s: None,
    })
}

/// Walks apps/files/settings and rebuilds the candidate table. The work runs on
/// the single index worker, so it cannot race the filesystem watcher; this
/// blocking-pool thread only waits for the result and reports it.
#[tauri::command]
pub async fn bootstrap_index(service: State<'_, IndexService>) -> Result<usize, String> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.full_index_blocking())
        .await
        .map_err(|err| err.to_string())?
}

/// Parks the running walk after its current entry. `true` when a walk was
/// running and is now paused; `false` when nothing was running or it was
/// already paused, so the UI never claims a pause that did not happen.
#[tauri::command(async)]
pub fn index_pause(service: State<'_, IndexService>) -> bool {
    service.pause()
}

#[tauri::command(async)]
pub fn index_resume(service: State<'_, IndexService>) -> bool {
    service.resume()
}

/// Stops the walk. The database writer skips its stale-row sweep, so the index
/// keeps every row the walk did not reach; the next complete walk reconciles it.
#[tauri::command(async)]
pub fn index_cancel(service: State<'_, IndexService>) -> bool {
    service.cancel()
}

/// The live switch state, for a UI that mounts while a walk is already parked.
/// `"idle"` means no walk is running — distinct from `"running"`, so a caller
/// never offers to pause nothing.
#[tauri::command(async)]
pub fn index_control_state(service: State<'_, IndexService>) -> String {
    match service.control_state() {
        None => "idle",
        Some(zeidex_indexing::IndexRun::Running) => "running",
        Some(zeidex_indexing::IndexRun::Paused) => "paused",
        Some(zeidex_indexing::IndexRun::Cancelled) => "cancelled",
    }
    .to_string()
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

/// Empties the candidate index. This is the Privacy screen's explicit "delete
/// index" action: it removes file and folder rows (and their usage counts) and
/// leaves settings, clipboard and recognized text alone. The next walk or
/// "Reindex now" repopulates it; until then search returns nothing.
#[tauri::command(async)]
pub fn index_clear(app: AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let removed = QueryEngine::clear_index(&state.db_path).map_err(|err| err.to_string())?;
    app.state::<IndexService>().reload();
    Ok(removed)
}

/// Deletes the launch history (which results were opened and when).
///
/// The Privacy screen offers this next to "delete index": the files stay
/// indexed and findable, but the ranking stops using what the user launched
/// before. Nothing else in the database is touched.
#[tauri::command(async)]
pub fn search_history_clear(app: AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let removed =
        QueryEngine::clear_usage_history(&state.db_path).map_err(|err| err.to_string())?;
    // The ranker reads the counters from the in-memory engine, not from the
    // database per query. Reloading here is what makes the action visible
    // before the next restart.
    state.reload_engine();
    Ok(removed)
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

    fn hit(path: &str, snippet: &str) -> DocumentHit {
        DocumentHit {
            path: path.to_string(),
            snippet: snippet.to_string(),
            page_count: 1,
            source: "ocr".to_string(),
        }
    }

    #[test]
    fn a_content_hit_never_duplicates_a_name_match() {
        let mut results = vec![LaunchResult {
            id: "file:/docs/report.pdf".to_string(),
            kind: "file".to_string(),
            title: "report.pdf".to_string(),
            subtitle: None,
            path: "/docs/report.pdf".to_string(),
            score: 120,
            action: LaunchResultAction::Open {
                path: "/docs/report.pdf".to_string(),
            },
            icon: None,
            match_kind: None,
            size_bytes: None,
            modified_unix_s: None,
        }];
        merge_document_hits(&mut results, vec![hit("/docs/report.pdf", "revenue grew")]);
        assert_eq!(results.len(), 1, "the same file is not listed twice");
        assert_eq!(results[0].score, 120, "and the scored row is untouched");
    }

    #[test]
    fn a_content_hit_is_openable_and_carries_its_excerpt() {
        let mut results = Vec::new();
        merge_document_hits(
            &mut results,
            vec![hit("/scans/invoice-2024.pdf", "المجموع 4,500")],
        );
        assert_eq!(results.len(), 1);
        let row = &results[0];
        assert_eq!(row.kind, "document");
        assert_eq!(row.title, "invoice-2024.pdf");
        assert_eq!(row.subtitle.as_deref(), Some("المجموع 4,500"));
        assert_eq!(row.id, "doc:/scans/invoice-2024.pdf");
        assert_eq!(row.score, 0, "below every name match");
        match &row.action {
            LaunchResultAction::Open { path } => assert_eq!(path, "/scans/invoice-2024.pdf"),
            other => panic!("expected an open action, got {other:?}"),
        }
    }
}
