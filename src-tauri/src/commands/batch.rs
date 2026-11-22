//! Batch Search: a pasted list of names, matched against the index.
//!
//! The matching itself lives in `zeidex-batch`; this module is the boundary: it
//! reads the indexed files once, refuses a list larger than the parser would
//! have produced, and writes the report. Nothing here talks to the network.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;
use zeidex_batch::matcher::{BatchFile, BatchReport, FileKind, search};
use zeidex_batch::parse::{MAX_ENTRIES, ParsedList, decode_text, parse_list};
use zeidex_batch::report::{ReportMeta, export_report};

use crate::state::AppState;

/// Splits pasted text into entries without touching the index.
///
/// Pure and cheap, so the review step can show what will be searched before the
/// user commits to a run.
#[tauri::command(async)]
pub fn batch_parse(text: String) -> ParsedList {
    parse_list(&text)
}

/// Reads a `.txt` list from disk and parses it.
///
/// The read happens here rather than in the webview so the size ceiling and the
/// encoding handling are the same for every import, and so the fs plugin does
/// not need a scope that would let the page read arbitrary files.
#[tauri::command(async)]
pub fn batch_import(path: String) -> Result<ParsedList, String> {
    const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;
    let metadata =
        std::fs::metadata(&path).map_err(|err| format!("could not read {path}: {err}"))?;
    if !metadata.is_file() {
        return Err(format!("{path} is not a file"));
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(format!(
            "{path} is larger than {} MiB; a list of names should be far smaller",
            MAX_IMPORT_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|err| format!("could not read {path}: {err}"))?;
    Ok(parse_list(&decode_text(&bytes)))
}

/// Matches the entries against every indexed file and folder.
#[tauri::command(async)]
pub fn batch_search(
    state: State<'_, AppState>,
    entries: Vec<String>,
    threshold: u8,
) -> Result<BatchReport, String> {
    // The parser caps a list; a caller that skips the parser is held to the same
    // ceiling rather than being allowed to freeze the window.
    if entries.len() > MAX_ENTRIES {
        return Err(format!(
            "{} entries is over the limit of {MAX_ENTRIES}",
            entries.len()
        ));
    }
    let files = indexed_files(&state)?;
    Ok(search(&files, &entries, threshold))
}

/// Writes the report the user is looking at, unchanged.
///
/// The report travels back from the UI instead of being recomputed: a re-run
/// would reflect an index that may have changed in between, and a report that
/// does not match the screen is worse than no report.
#[tauri::command(async)]
pub fn batch_export(report: BatchReport, path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("no report path was given".to_string());
    }
    let generated_unix_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    let meta = ReportMeta {
        generated_unix_s,
        app_version: env!("CARGO_PKG_VERSION"),
    };
    export_report(&report, &meta, &path).map_err(|err| err.to_string())
}

/// The indexed files and folders, in the shape the matcher wants.
///
/// The store lock is held only for the read; the matching runs after it is
/// released, so a long batch cannot block a clipboard write or an index refresh.
fn indexed_files(state: &State<'_, AppState>) -> Result<Vec<BatchFile>, String> {
    let rows = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store.load_indexed_paths().map_err(|err| err.to_string())?
    };
    Ok(rows
        .into_iter()
        .map(|row| {
            let kind = if row.is_folder {
                FileKind::Folder
            } else {
                FileKind::File
            };
            BatchFile::new(row.path, row.name, kind, row.modified_unix_s)
        })
        .collect())
}
