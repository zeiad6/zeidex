//! Document text the launcher can search.
//!
//! The Studio recognizes a scan, the user exports it — and the text is gone.
//! These commands keep that text in the local database so the launcher can find
//! the file by what is inside it, offline. Nothing here talks to the network:
//! the body arrives from the OCR engine that just ran on this machine.
//!
//! Storing is off the UI thread (`async`) because a 200-page document is a
//! multi-megabyte string and its tokenization is a real amount of work.

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// How many documents the index holds, plus the switch that decides whether a
/// finished run is stored at all.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentIndexStatus {
    pub enabled: bool,
    pub count: usize,
}

#[tauri::command(async)]
pub fn document_index_status(state: State<'_, AppState>) -> Result<DocumentIndexStatus, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(DocumentIndexStatus {
        enabled: store
            .document_indexing_enabled()
            .map_err(|err| err.to_string())?,
        count: store.document_text_count().map_err(|err| err.to_string())?,
    })
}

/// Turn indexing of recognized text on or off. Existing rows are kept either
/// way; clearing is a separate, explicit action.
#[tauri::command(async)]
pub fn document_set_indexing(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .set_document_indexing_enabled(enabled)
        .map_err(|err| err.to_string())?;
    Ok(enabled)
}

/// The file's modification time, when the filesystem reports one.
///
/// Only the staleness pass consumes this, so a file whose mtime cannot be read
/// is still indexed: the cost is one entry that is never pruned, not a document
/// the user cannot search.
fn modified_at(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

/// Store the text of one file. Returns the number of searchable documents, so
/// the caller can show progress without a second round trip.
///
/// An empty body removes the row: a page that produced no text must not shadow
/// an older, useful entry for the same path.
#[tauri::command(async)]
pub fn document_text_store(
    state: State<'_, AppState>,
    path: String,
    body: String,
    page_count: Option<usize>,
    source: Option<String>,
) -> Result<usize, String> {
    // Authorized before the store lock: `authorize_read` takes that lock itself,
    // and `std::sync::Mutex` is not reentrant.
    let path = state.authorize_read(&path)?;
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !store
        .document_indexing_enabled()
        .map_err(|err| err.to_string())?
    {
        // Off means off: the run is not stored, and the caller can tell because
        // the count did not move.
        return store.document_text_count().map_err(|err| err.to_string());
    }
    let document = zeidex_storage::DocumentText {
        path: path.to_string_lossy().into_owned(),
        body,
        page_count: page_count.unwrap_or(1),
        source: source.unwrap_or_else(|| "ocr".to_string()),
        modified_at_unix_s: modified_at(&path),
    };
    store
        .upsert_document_text(&document)
        .map_err(|err| err.to_string())?;
    store.document_text_count().map_err(|err| err.to_string())
}

#[tauri::command(async)]
pub fn document_text_remove(state: State<'_, AppState>, path: String) -> Result<bool, String> {
    let path = state.authorize_read(&path)?;
    let path = path.to_string_lossy();
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .remove_document_text(&path)
        .map_err(|err| err.to_string())
}

/// Forget every stored text. The user's files are untouched.
#[tauri::command(async)]
pub fn document_text_clear(state: State<'_, AppState>) -> Result<usize, String> {
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.clear_document_text().map_err(|err| err.to_string())
}
