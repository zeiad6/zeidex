//! Batch Search: a pasted list of names, matched against the index.
//!
//! The matching itself lives in `zeidex-batch`; this module is the boundary: it
//! reads the indexed files once, refuses a list larger than the parser would
//! have produced, and writes the report. Nothing here talks to the network.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};
use zeidex_batch::matcher::{BatchFile, BatchReport, FileKind, search};
use zeidex_batch::parse::{MAX_ENTRIES, ParsedList, decode_text, parse_list};
use zeidex_batch::report::{ReportMeta, export_report};

use super::dialogs;
use crate::state::AppState;

/// Most matches one request may carry back to the report.
///
/// The matcher returns eight; the export accepts a report produced by the UI, so
/// it needs a ceiling of its own rather than trusting the caller.
const MAX_MATCHES_PER_ENTRY: usize = 256;

/// Splits pasted text into entries without touching the index.
///
/// Pure and cheap, so the review step can show what will be searched before the
/// user commits to a run.
#[tauri::command(async)]
pub fn batch_parse(text: String) -> ParsedList {
    parse_list(&text)
}

/// Asks the user for a `.txt` list and parses it.
///
/// The picker is opened from Rust, not from the webview, so the page never
/// chooses which path is read: the only file this command will open is the one
/// the user just selected in a native dialog. Returns `None` when the dialog was
/// dismissed, which is not an error.
///
/// `title` and `filter_name` are cosmetic and come from the UI so the dialog
/// speaks the user's language.
#[tauri::command(async)]
pub fn batch_import(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<ParsedList>, String> {
    const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;
    let Some(path) = dialogs::pick_file(&app, &state.paths, title, filter_name, &["txt"])? else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(format!(
            "{} is larger than {} MiB; a list of names should be far smaller",
            path.display(),
            MAX_IMPORT_BYTES / (1024 * 1024)
        ));
    }
    let bytes =
        std::fs::read(&path).map_err(|err| format!("could not read {}: {err}", path.display()))?;
    Ok(Some(parse_list(&decode_text(&bytes))))
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

/// Asks the user where to save, then writes the report they are looking at.
///
/// The report travels back from the UI instead of being recomputed: a re-run
/// would reflect an index that may have changed in between, and a report that
/// does not match the screen is worse than no report. It is therefore validated
/// first — the webview is a caller like any other, and a hand-built report with
/// a million rows would be a way to hang the export thread.
///
/// The workbook is written beside the target and renamed into place, so a failed
/// export leaves the previous file intact instead of truncating it. Returns the
/// path actually written, or `None` when the dialog was dismissed.
#[tauri::command(async)]
pub fn batch_export(
    app: AppHandle,
    report: BatchReport,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    validate_report(&report)?;

    let generated_unix_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    let (year, month, day) = utc_date(generated_unix_s);
    let default_name = format!("zeidex-batch-{year:04}-{month:02}-{day:02}.xlsx");

    let Some(target) = dialogs::save_file(
        &app,
        title,
        filter_name,
        "xlsx",
        Some(default_name.as_str()),
    )?
    else {
        return Ok(None);
    };

    let meta = ReportMeta {
        generated_unix_s,
        app_version: env!("CARGO_PKG_VERSION"),
    };
    dialogs::write_atomically(&target, |path| {
        export_report(&report, &meta, path).map_err(|err| err.to_string())
    })?;
    Ok(Some(target.display().to_string()))
}

/// Rejects a report the UI could not have produced.
///
/// The caps mirror the ones the search already enforces, so an honest report
/// always passes and a hand-built one cannot make the export allocate without
/// bound.
fn validate_report(report: &BatchReport) -> Result<(), String> {
    if report.threshold > 100 {
        return Err(format!(
            "the report claims a {}% threshold; the slider stops at 100",
            report.threshold
        ));
    }
    if report.entries.len() > MAX_ENTRIES {
        return Err(format!(
            "the report has {} rows, over the limit of {MAX_ENTRIES}",
            report.entries.len()
        ));
    }
    for entry in &report.entries {
        if entry.matches.len() > MAX_MATCHES_PER_ENTRY {
            return Err(format!(
                "{} carries {} matches, over the limit of {MAX_MATCHES_PER_ENTRY}",
                entry.requested,
                entry.matches.len()
            ));
        }
        if entry.matches.iter().any(|hit| hit.score > 100) {
            return Err(format!("{} carries a score above 100%", entry.requested));
        }
    }
    Ok(())
}

/// The UTC calendar date of a Unix timestamp, as `(year, month, day)`.
///
/// Only used to name the default file; a dependency on a date library for one
/// `format!` would not pay for itself. This is Howard Hinnant's `civil_from_days`,
/// which is exact for every timestamp the app can produce.
fn utc_date(unix_s: i64) -> (i64, u32, u32) {
    let days = unix_s.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The indexed files and folders, in the shape the matcher wants.
///
/// The store lock is held only for the read, and the matching runs after it is
/// released, so a long batch cannot block a clipboard write or an index refresh.
///
/// The result is cached against the store's change counter. Building it costs a
/// query plus five strings and two normalisations per indexed file, and the batch
/// screen re-searches every time the threshold slider moves; on a six-figure
/// index that is a visible stall between slider steps. The cache key is SQLite's
/// own counter, so any write at all invalidates it — a rebuild that was not
/// needed is cheap, a stale answer would not be.
fn indexed_files(state: &State<'_, AppState>) -> Result<Arc<Vec<BatchFile>>, String> {
    let (version, rows) = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let version = store.data_version();
        if let Some((cached_version, cached)) = state
            .batch_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            && *cached_version == version
        {
            return Ok(Arc::clone(cached));
        }
        let rows = store.load_indexed_paths().map_err(|err| err.to_string())?;
        (version, rows)
    };

    let files: Arc<Vec<BatchFile>> = Arc::new(
        rows.into_iter()
            .map(|row| {
                let kind = if row.is_folder {
                    FileKind::Folder
                } else {
                    FileKind::File
                };
                BatchFile::new(row.path, row.name, kind, row.modified_unix_s)
            })
            .collect(),
    );
    *state
        .batch_files
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((version, Arc::clone(&files)));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{utc_date, validate_report};
    use crate::commands::dialogs::with_extension;
    use zeidex_batch::matcher::{
        BatchEntryResult, BatchMatch, BatchReport, BatchSummary, FileKind,
    };

    fn report(threshold: u8, entries: Vec<BatchEntryResult>) -> BatchReport {
        BatchReport {
            threshold,
            indexed_files: 1,
            entries,
            summary: BatchSummary::default(),
        }
    }

    fn entry(requested: &str, matches: Vec<BatchMatch>) -> BatchEntryResult {
        BatchEntryResult {
            requested: requested.to_string(),
            status: zeidex_batch::matcher::BatchStatus::NotFound,
            total_matches: matches.len(),
            matches,
            duration_ms: 0,
        }
    }

    fn hit(score: u8) -> BatchMatch {
        BatchMatch {
            path: "/tmp/report.pdf".to_string(),
            name: "report.pdf".to_string(),
            extension: Some("pdf".to_string()),
            size_bytes: Some(10),
            modified_unix_s: Some(0),
            kind: FileKind::File,
            score,
            match_type: zeidex_batch::matcher::MatchKind::Fuzzy,
        }
    }

    #[test]
    fn a_date_is_named_correctly_across_a_leap_day() {
        // 2024-02-29, 2024-03-01, 1970-01-01 and 2026-11-22 12:00:00 UTC.
        assert_eq!(utc_date(1_709_164_800), (2024, 2, 29));
        assert_eq!(utc_date(1_709_251_200), (2024, 3, 1));
        assert_eq!(utc_date(0), (1970, 1, 1));
        assert_eq!(utc_date(1_795_358_400), (2026, 11, 22));
        // A timestamp before the epoch must not produce a negative day count
        // dressed up as a valid date.
        assert_eq!(utc_date(-1), (1969, 12, 31));
    }

    #[test]
    fn the_report_gets_the_extension_the_workbook_needs() {
        assert_eq!(
            with_extension(std::path::PathBuf::from("/tmp/report"), "xlsx"),
            std::path::PathBuf::from("/tmp/report.xlsx")
        );
        assert_eq!(
            with_extension(std::path::PathBuf::from("/tmp/report.xlsx"), "xlsx"),
            std::path::PathBuf::from("/tmp/report.xlsx")
        );
    }

    #[test]
    fn a_report_the_ui_could_not_have_produced_is_refused() {
        assert!(validate_report(&report(70, vec![entry("a.pdf", vec![hit(70)])])).is_ok());
        assert!(validate_report(&report(101, vec![])).is_err());
        assert!(validate_report(&report(70, vec![entry("a.pdf", vec![hit(101)])])).is_err());
        let too_many = (0..super::MAX_MATCHES_PER_ENTRY + 1)
            .map(|_| hit(80))
            .collect::<Vec<_>>();
        assert!(validate_report(&report(70, vec![entry("a.pdf", too_many)])).is_err());
    }
}
