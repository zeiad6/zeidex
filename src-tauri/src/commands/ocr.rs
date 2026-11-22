use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State, Window};
use zeidex_ocr::{PageResult, PipelineProgress};

use super::dialogs;
use crate::state::AppState;

/// Every format the Studio can open, in one list.
///
/// The picker's filter, the engine's readers and the frontend's drop check must
/// agree: a format the dialog offers but the run refuses is a dead end, and one
/// the drop handler refuses but the dialog offers is a dead end too. The dialog
/// takes this list from Rust so the page cannot widen it; the TypeScript copy is
/// checked against this array by `documents.test.ts`.
pub const SUPPORTED_DOCUMENT_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp", "pdf", "docx", "xlsx", "odt", "txt", "md",
    "markdown", "csv", "tsv", "log",
];

/// Asks the user for a document and returns the path.
///
/// The dialog runs here, not in the webview, so the page cannot choose what the
/// next command reads. The chosen path is minted into the path policy by the
/// picker, which is what makes `ocr_process_image`/`ocr_process_pdf` accept it.
/// Returns `None` when the dialog was dismissed, which is not an error.
#[tauri::command(async)]
pub fn ocr_pick_file(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let picked = dialogs::pick_file(
        &app,
        &state.paths,
        title,
        filter_name,
        SUPPORTED_DOCUMENT_EXTENSIONS,
    )?;
    Ok(picked.map(|path| path.display().to_string()))
}

#[derive(Debug, Serialize)]
pub struct ModelsStatus {
    pub dir: String,
    pub complete: bool,
    pub missing: Vec<String>,
    /// The pack the next OCR run will load. A downloaded pack replaces the
    /// bundled detection and recognition models; layout and table stay shared.
    pub pack_id: &'static str,
    pub pack_name: &'static str,
    pub pack_name_ar: &'static str,
    pub bundled: bool,
    pub bundled_dir: String,
    pub models_root: String,
}

/// A page rendered for the Studio's preview pane.
#[derive(Debug, Serialize)]
pub struct PagePreview {
    pub data_url: String,
    /// Full-resolution page dimensions; region boxes are expressed in this space.
    pub width: u32,
    pub height: u32,
}

/// Renders a page as a PNG data URL so the Studio can show the scan beside the
/// extracted text without an asset-protocol scope.
#[tauri::command]
pub async fn ocr_page_preview(
    state: State<'_, AppState>,
    path: String,
    page: Option<usize>,
    max_side: Option<u32>,
) -> Result<PagePreview, String> {
    let path = state.authorize_read(&path)?;
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let page = page.unwrap_or(0);
    // Bounded: the page is rendered at this size and then base64-encoded, which
    // multiplies the cost, so an unbounded value from a bad caller could
    // exhaust memory. 4000 px still covers an A4 page at 300 dpi.
    let max_side = max_side.unwrap_or(1400).clamp(200, 4000);
    let preview =
        tauri::async_runtime::spawn_blocking(move || engine.page_preview(&path, page, max_side))
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| err.to_string())?;

    let encoded = BASE64.encode(&preview.png);
    Ok(PagePreview {
        data_url: format!("data:image/png;base64,{encoded}"),
        width: preview.width,
        height: preview.height,
    })
}

/// Exports every recognized page into one `.docx`, separated by page breaks.
///
/// The pages arrive as JSON from the Studio, which keeps the OCR result editable
/// before export. The save dialog is opened here, not in the webview: a command
/// that writes to a path the page supplies is a way to overwrite a file the user
/// never chose. Returns the path written, or `None` when the dialog was
/// dismissed.
///
/// `(async)`: packing a document is a zip write to disk, and a command without
/// it does that on the main thread.
#[tauri::command(async)]
pub fn export_docx_pages(
    app: AppHandle,
    state: State<'_, AppState>,
    pages: Vec<PageResult>,
    default_name: Option<String>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let Some(target) =
        dialogs::save_file(&app, title, filter_name, "docx", default_name.as_deref())?
    else {
        return Ok(None);
    };
    dialogs::write_atomically(&target, |path| {
        zeidex_exporter::docx::export_docx_pages(&pages, path).map_err(|err| err.to_string())
    })?;
    // The shell wrote it, so opening it afterwards needs no second pick.
    state.paths.mint(&target);
    Ok(Some(target.display().to_string()))
}

/// Exports every recognized page as its own worksheet in one `.xlsx`.
#[tauri::command(async)]
pub fn export_xlsx_pages(
    app: AppHandle,
    state: State<'_, AppState>,
    pages: Vec<PageResult>,
    default_name: Option<String>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let Some(target) =
        dialogs::save_file(&app, title, filter_name, "xlsx", default_name.as_deref())?
    else {
        return Ok(None);
    };
    dialogs::write_atomically(&target, |path| {
        zeidex_exporter::xlsx::export_xlsx_pages(&pages, path).map_err(|err| err.to_string())
    })?;
    state.paths.mint(&target);
    Ok(Some(target.display().to_string()))
}

/// Which model files the active pack needs, so the OCR Studio can show a clear
/// message instead of a runtime error when a pack is incomplete.
#[tauri::command(async)]
pub fn ocr_models_status(state: State<'_, AppState>) -> Result<ModelsStatus, String> {
    let pack = state.active_pack();
    let paths = state.ocr_paths();
    let dir = paths
        .det
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(ModelsStatus {
        dir,
        complete: paths.is_complete(),
        missing: paths.missing(),
        pack_id: pack.id,
        pack_name: pack.name,
        pack_name_ar: pack.name_ar,
        bundled: pack.bundled,
        bundled_dir: state.models_dir.to_string_lossy().into_owned(),
        models_root: state.models_root.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn ocr_process_image(
    state: State<'_, AppState>,
    path: String,
) -> Result<PageResult, String> {
    let path = state.authorize_read(&path)?;
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let result = tauri::async_runtime::spawn_blocking(move || engine.process_image(&path))
        .await
        .map_err(|err| err.to_string())?;
    result.map_err(|err| err.to_string())
}

/// Extracts the text of a `.docx`, `.xlsx` or plain text file.
///
/// These formats already hold characters, so OCR is the wrong tool: running a
/// detector over a Word file's bytes would recognize nothing. The reader is in
/// `zeidex-ocr` (`document.rs`), and it returns the same `PageResult` the
/// pipeline returns — the Studio's text pane, export and content index need no
/// second code path, and nothing is sent anywhere.
#[tauri::command]
pub async fn document_extract_text(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<PageResult>, String> {
    let path = state.authorize_read(&path)?;
    tauri::async_runtime::spawn_blocking(move || zeidex_ocr::extract_document(&path))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

/// Processes a PDF, emitting `ocr-progress` events the Studio listens to.
///
/// Every PDF run takes the cancellation slot, so `ocr_cancel` always refers to
/// the run the user is watching. The slot is released on both paths, including
/// a join failure, so a panic cannot leave a dead token behind.
#[tauri::command]
pub async fn ocr_process_pdf(
    state: State<'_, AppState>,
    path: String,
    window: Window,
) -> Result<Vec<PageResult>, String> {
    let path = state.authorize_read(&path)?;
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let token = state.begin_ocr_run();
    let finished = token.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let progress = |event: PipelineProgress| {
            let _ = window.emit("ocr-progress", event);
        };
        engine.process_pdf_cancellable(&path, &token, &progress)
    })
    .await;
    state.end_ocr_run(&finished);
    outcome
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

/// Processes a 1-based, inclusive page window of a PDF, emitting the same
/// `ocr-progress` events as a full run. The Studio uses this to keep a long
/// scan bounded to the pages the user actually needs.
#[tauri::command]
pub async fn ocr_process_pdf_range(
    state: State<'_, AppState>,
    path: String,
    first_page: usize,
    last_page: usize,
    window: Window,
) -> Result<Vec<PageResult>, String> {
    let path = state.authorize_read(&path)?;
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let token = state.begin_ocr_run();
    let finished = token.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let progress = |event: PipelineProgress| {
            let _ = window.emit("ocr-progress", event);
        };
        engine.process_pdf_range_cancellable(&path, first_page, last_page, &token, &progress)
    })
    .await;
    state.end_ocr_run(&finished);
    outcome
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

/// Asks the running recognition to stop between pages.
///
/// Returns whether a run was live, so the Studio can distinguish "cancelled"
/// from "the run finished before the click landed". A page already inside ONNX
/// Runtime still completes; the pipeline stops before starting the next one.
#[tauri::command]
pub fn ocr_cancel(state: State<'_, AppState>) -> bool {
    state.cancel_ocr_run()
}
