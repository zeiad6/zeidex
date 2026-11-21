use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;
use zeidex_storage::{CLIPBOARD_KIND_IMAGE, CLIPBOARD_KIND_TEXT, ClipboardEntry};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ClipDto {
    pub id: i64,
    pub content: String,
    pub kind: String,
    pub copied_at_unix_s: i64,
    pub copy_count: i64,
    pub pinned: bool,
}

impl From<ClipboardEntry> for ClipDto {
    fn from(entry: ClipboardEntry) -> Self {
        Self {
            id: entry.id,
            content: entry.content,
            kind: entry.kind,
            copied_at_unix_s: entry.copied_at_unix_s,
            copy_count: entry.copy_count,
            pinned: entry.pinned,
        }
    }
}

#[tauri::command]
pub fn clipboard_list(
    state: State<'_, AppState>,
    kind: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ClipDto>, String> {
    let kind = normalize_kind(&kind)?;
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = store
        .clipboard_entries(&kind, &query, limit.unwrap_or(100))
        .map_err(|err| err.to_string())?;
    Ok(entries.into_iter().map(ClipDto::from).collect())
}

/// Records a clip the UI already holds (for example text the launcher itself
/// copied on the user's behalf).
#[tauri::command]
pub fn clipboard_record_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<Option<i64>, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .record_clipboard_entry(&text, None)
        .map_err(|err| err.to_string())
}

/// Polls the system pasteboard. Clipboard APIs expose no change event, so the
/// UI calls this on an interval; the state guard keeps a poll from re-recording
/// the clip that is already on the pasteboard.
#[tauri::command]
pub fn clipboard_poll(state: State<'_, AppState>) -> Result<Option<ClipDto>, String> {
    let text = read_clipboard_text()?;

    {
        let mut last = state
            .last_clipboard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.as_deref() == Some(text.as_str()) || text.trim().is_empty() {
            return Ok(None);
        }
        // Set before the write so a failed insert cannot be retried on every
        // poll; the next real copy replaces it.
        *last = Some(text.clone());
    }

    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = store
        .record_clipboard_entry(&text, None)
        .map_err(|err| err.to_string())?;

    // The UI re-reads the list for the authoritative row; this carries just
    // enough to show the new clip immediately.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    Ok(id.map(|id| ClipDto {
        id,
        content: text.clone(),
        kind: CLIPBOARD_KIND_TEXT.to_string(),
        copied_at_unix_s: now,
        copy_count: 1,
        pinned: false,
    }))
}

#[tauri::command]
pub fn clipboard_delete(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .delete_clipboard_entry(id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn clipboard_set_pinned(
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<bool, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .set_clipboard_pinned(id, pinned)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn clipboard_clear(state: State<'_, AppState>) -> Result<usize, String> {
    {
        let mut last = state
            .last_clipboard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *last = None;
    }
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .clear_clipboard_entries()
        .map_err(|err| err.to_string())
}

fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    clipboard.get_text().map_err(|err| err.to_string())
}

fn normalize_kind(kind: &str) -> Result<String, String> {
    match kind {
        "text" => Ok(CLIPBOARD_KIND_TEXT.to_string()),
        "image" => Ok(CLIPBOARD_KIND_IMAGE.to_string()),
        other => Err(format!("unknown clipboard kind: {other}")),
    }
}
