use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use tauri::{Emitter, State, Window};
use zeidex_ocr::{PageResult, PipelineProgress};

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ModelsStatus {
    pub dir: String,
    pub complete: bool,
    pub missing: Vec<String>,
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
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let page = page.unwrap_or(0);
    // Bounded: the page is rendered at this size and then base64-encoded, which
    // multiplies the cost, so an unbounded value from a bad caller could
    // exhaust memory. 4000 px still covers an A4 page at 300 dpi.
    let max_side = max_side.unwrap_or(1400).clamp(200, 4000);
    let preview = tauri::async_runtime::spawn_blocking(move || {
        engine.page_preview(Path::new(&path), page, max_side)
    })
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

/// Exports a recognized page to `.docx`. The page arrives as JSON from the
/// Studio, which keeps the OCR result editable before export.
///
/// `(async)`: packing a document is a zip write to disk, and a command without
/// it does that on the main thread.
#[tauri::command(async)]
pub fn export_docx(page: PageResult, path: String) -> Result<(), String> {
    zeidex_exporter::docx::export_docx(&page, Path::new(&path)).map_err(|err| err.to_string())
}

/// Exports every recognized page into one `.docx`, separated by page breaks.
#[tauri::command(async)]
pub fn export_docx_pages(pages: Vec<PageResult>, path: String) -> Result<(), String> {
    zeidex_exporter::docx::export_docx_pages(&pages, Path::new(&path))
        .map_err(|err| err.to_string())
}

#[tauri::command(async)]
pub fn export_xlsx(page: PageResult, path: String) -> Result<(), String> {
    zeidex_exporter::xlsx::export_xlsx(&page, Path::new(&path)).map_err(|err| err.to_string())
}

/// Exports every recognized page as its own worksheet in one `.xlsx`.
#[tauri::command(async)]
pub fn export_xlsx_pages(pages: Vec<PageResult>, path: String) -> Result<(), String> {
    zeidex_exporter::xlsx::export_xlsx_pages(&pages, Path::new(&path))
        .map_err(|err| err.to_string())
}

/// Which model files are present, so the OCR Studio can show a clear message
/// instead of a runtime error when the user has not run `fetch_models.sh`.
#[tauri::command(async)]
pub fn ocr_models_status(state: State<'_, AppState>) -> Result<ModelsStatus, String> {
    let paths = zeidex_ocr::onnx::ModelPaths::from_dir(&state.models_dir);
    Ok(ModelsStatus {
        dir: state.models_dir.to_string_lossy().into_owned(),
        complete: paths.is_complete(),
        missing: paths.missing(),
    })
}

#[tauri::command]
pub async fn ocr_process_image(
    state: State<'_, AppState>,
    path: String,
) -> Result<PageResult, String> {
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let path = path.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || engine.process_image(Path::new(&path)))
            .await
            .map_err(|err| err.to_string())?;
    result.map_err(|err| err.to_string())
}

/// Processes a PDF, emitting `ocr-progress` events the Studio listens to.
#[tauri::command]
pub async fn ocr_process_pdf(
    state: State<'_, AppState>,
    path: String,
    window: Window,
) -> Result<Vec<PageResult>, String> {
    let engine = state.ocr_engine().map_err(|err| err.to_string())?;
    let path = path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let progress = |event: PipelineProgress| {
            let _ = window.emit("ocr-progress", event);
        };
        engine.process_pdf(Path::new(&path), &progress)
    })
    .await
    .map_err(|err| err.to_string())?;
    result.map_err(|err| err.to_string())
}
