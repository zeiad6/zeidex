use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};
use zeidex_storage::{
    CLIPBOARD_KIND_IMAGE, CLIPBOARD_KIND_TEXT, ClipboardEntry, MAX_CLIPBOARD_ROWS,
};

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

/// The list is read off the main thread: it holds up to `MAX_CLIPBOARD_ROWS`
/// clips of `MAX_CLIPBOARD_ENTRY_BYTES` each, and serializing that on the UI
/// thread is a visible stall.
#[tauri::command(async)]
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
    // The table cannot hold more than this, so a larger `limit` is a request the
    // query would scan for nothing. The bound matters because each row carries up
    // to `MAX_CLIPBOARD_ENTRY_BYTES` of text.
    let entries = store
        .clipboard_entries(&kind, &query, limit.unwrap_or(100).min(MAX_CLIPBOARD_ROWS))
        .map_err(|err| err.to_string())?;
    Ok(entries.into_iter().map(ClipDto::from).collect())
}

/// Records a clip the UI already holds (for example text the launcher itself
/// copied on the user's behalf). Pausing blocks this too: the switch means "do
/// not write what I copy", and the launcher's own copies are still copies.
#[tauri::command(async)]
pub fn clipboard_record_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<Option<i64>, String> {
    // Same lock order as every other clipboard command: watermark, then store.
    // This command is used after the launcher puts text on the pasteboard, so
    // the watermark has to move with it: the monitor polls that same pasteboard
    // and would otherwise see this clip as new, rewrite the row it just created
    // (new id, inflated copy count) on its next tick.
    let mut watermark = state
        .last_clipboard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let paused = store.clipboard_paused().map_err(|err| err.to_string())?;
    let id = if paused {
        None
    } else {
        store
            .record_clipboard_entry(&text, None)
            .map_err(|err| err.to_string())?
    };
    // Marked seen even when paused: the clip is on the pasteboard either way,
    // and a later resume must not write this one.
    *watermark = Some(text);
    Ok(id)
}

/// Whether the history is currently paused.
#[tauri::command(async)]
pub fn clipboard_paused(state: State<'_, AppState>) -> Result<bool, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.clipboard_paused().map_err(|err| err.to_string())
}

/// Pauses or resumes recording.
///
/// On resume the current pasteboard is read into the watermark before the flag
/// is cleared, so a clip copied during the pause is not written by the poll that
/// follows. See `poll_once` for why the watermark lock is taken first.
#[tauri::command(async)]
pub fn clipboard_set_paused(state: State<'_, AppState>, paused: bool) -> Result<bool, String> {
    // Lock order is `last_clipboard` then `store` everywhere (see `AppState`);
    // holding the watermark across the read and the flag write is what makes a
    // resume atomic with respect to an in-flight poll.
    let mut watermark = state
        .last_clipboard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !paused {
        match read_clipboard_text() {
            Ok(Some(current)) => *watermark = Some(current),
            // Nothing textual is on the pasteboard, so the next text copied is
            // new by definition.
            Ok(None) => *watermark = None,
            // The pasteboard could not be read, so there is no value to absorb.
            // Arm the one-shot skip: the next clip is treated as the watermark
            // rather than written, because it may be the one copied while
            // recording was off.
            Err(_) => state.clipboard_skip_next.store(true, Ordering::SeqCst),
        }
    }

    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .set_clipboard_paused(paused)
        .map_err(|err| err.to_string())?;
    Ok(paused)
}

/// One poll of the system pasteboard. Clipboard APIs expose no change event, so
/// something has to poll; the watermark keeps a poll from re-recording the clip
/// that is already on the pasteboard.
///
/// The watermark is updated while paused too — only the write is skipped. That
/// is what makes the pause hold: whatever was copied during it is already
/// "seen" when recording resumes.
pub(crate) fn poll_once(state: &AppState) -> Result<(), String> {
    // The watermark guard is held across the read, the comparison and the
    // write, so a pause, a resume or a clear cannot interleave between them.
    // `last_clipboard` therefore outlives every other lock here.
    let mut watermark = state
        .last_clipboard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(text) = read_clipboard_text()? else {
        // The pasteboard holds an image or a file list, not text. Clearing the
        // watermark is what makes the next text a new copy: that text is no
        // longer what the pasteboard holds, so copying it again must lift its
        // row instead of looking like the clip that never moved.
        *watermark = None;
        return Ok(());
    };
    // A resume that could not read the pasteboard left this armed; the first
    // clip seen afterwards is absorbed as the watermark, not recorded.
    if state.clipboard_skip_next.swap(false, Ordering::SeqCst) {
        *watermark = Some(text);
        return Ok(());
    }
    if watermark.as_deref() == Some(text.as_str()) {
        return Ok(());
    }
    // Set before the write so a failed insert cannot be retried on every poll;
    // the next real copy replaces it. This is also set for whitespace-only
    // text, which is not worth a row: otherwise copying A, then a blank line,
    // then A again would leave A looking like it was never re-copied.
    *watermark = Some(text.clone());
    if text.trim().is_empty() {
        return Ok(());
    }
    // `watermark` stays alive to the end of the function on purpose: the pause
    // check below has to happen while it is still held, or a resume could land
    // in between and let this pre-pause clip through.

    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if store.clipboard_paused().map_err(|err| err.to_string())? {
        return Ok(());
    }
    // The row the store writes is what the UI lists; the poll carries no copy of
    // it, so the list stays the single source of truth for ids and counts.
    store
        .record_clipboard_entry(&text, None)
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Polls the pasteboard once, for the UI. The background monitor below does the
/// recording; this exists so opening the history shows a clip copied just before
/// the tab was opened instead of waiting for the next monitor tick.
#[tauri::command(async)]
pub fn clipboard_poll(state: State<'_, AppState>) -> Result<(), String> {
    poll_once(&state)
}

/// How often the background monitor reads the pasteboard. A read that finds the
/// same text costs one pasteboard round-trip and no database work.
const CLIPBOARD_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Records clips for as long as the app runs, independent of the webview.
///
/// This used to be the UI's `setInterval` on the clipboard tab. Webviews throttle
/// timers in hidden windows and the launcher spends most of its life hidden, so
/// capture only happened while that tab was on screen. Keeping the poll here also
/// means a pasteboard that blocks (X11 waits for the selection owner) cannot stall
/// the event loop.
pub fn run_monitor(app: AppHandle) {
    // A pasteboard that stays unreadable must not print once a second: report
    // each distinct failure once, and clear it as soon as a poll succeeds.
    let mut last_error: Option<String> = None;
    loop {
        std::thread::sleep(CLIPBOARD_POLL_INTERVAL);
        let Some(state) = app.try_state::<AppState>() else {
            // The app is shutting down; there is nothing left to record into.
            return;
        };
        match poll_once(&state) {
            Ok(_) => last_error = None,
            Err(err) => {
                if last_error.as_deref() != Some(err.as_str()) {
                    eprintln!("zeidex: clipboard monitor: {err}");
                    last_error = Some(err);
                }
            }
        }
    }
}

#[tauri::command(async)]
pub fn clipboard_delete(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .delete_clipboard_entry(id)
        .map_err(|err| err.to_string())
}

#[tauri::command(async)]
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

/// Empties the history. The clip currently on the pasteboard is marked as seen
/// first: otherwise the poller would put it straight back a second later.
#[tauri::command(async)]
pub fn clipboard_clear(state: State<'_, AppState>) -> Result<usize, String> {
    // `last_clipboard` before `store`, like every other clipboard command.
    let mut watermark = state
        .last_clipboard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(Some(current)) = read_clipboard_text() {
        *watermark = Some(current);
    }

    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .clear_clipboard_entries()
        .map_err(|err| err.to_string())
}

/// The pasteboard's text, or `None` when it holds something else (an image, a
/// file list). "No text" is a normal state, not an error: the poller runs every
/// second and treating it as a failure made a copied screenshot look like the
/// clipboard was broken.
fn read_clipboard_text() -> Result<Option<String>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    match clipboard.get_text() {
        Ok(text) => Ok(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

fn normalize_kind(kind: &str) -> Result<String, String> {
    match kind {
        "text" => Ok(CLIPBOARD_KIND_TEXT.to_string()),
        "image" => Ok(CLIPBOARD_KIND_IMAGE.to_string()),
        other => Err(format!("unknown clipboard kind: {other}")),
    }
}
