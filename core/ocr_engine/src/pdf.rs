use std::path::Path;

use image::DynamicImage;
pub use pdfium_render::prelude::Pdfium;
use pdfium_render::prelude::*;

use crate::error::{OcrError, OcrResult};

/// Binds to a bundled pdfium shared library when `library_path` is given,
/// otherwise to the system library. `pdfium-render` loads the library at
/// runtime, so the binary has no link-time dependency.
pub fn bind_pdfium(library_path: Option<&Path>) -> OcrResult<Pdfium> {
    let bindings = match library_path {
        Some(path) => Pdfium::bind_to_library(path),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|err| OcrError::Pdf(err.to_string()))?;
    Ok(Pdfium::new(bindings))
}

pub fn page_count(document: &PdfDocument<'_>) -> i32 {
    document.pages().len()
}

/// Renders one page at `dpi` (pdfium's default is 72). 300 DPI is the
/// blueprint's target for OCR-quality scans.
pub fn render_document_page(
    document: &PdfDocument<'_>,
    page_index: i32,
    dpi: f32,
) -> OcrResult<DynamicImage> {
    let page = document
        .pages()
        .get(page_index)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(dpi / 72.0)
        .set_format(PdfBitmapFormat::BGR);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|err| OcrError::Pdf(err.to_string()))?;
    bitmap
        .as_image()
        .map_err(|err| OcrError::Pdf(err.to_string()))
}
