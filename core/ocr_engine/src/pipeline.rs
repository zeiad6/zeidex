use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;

use crate::bidi;
use crate::error::{OcrError, OcrResult};
use crate::geometry::BBox;
use crate::layout::{self, PageLayout, Region, RegionKind};
use crate::onnx::{self, ModelPaths, ModelSet};
use crate::pdf::{self, Pdfium};
use crate::table;

/// Configuration for [`OcrEngine`]. `models_dir` is where the downloader puts
/// the ONNX files; `pdfium_library` is the bundled shared library, if any.
pub struct OcrEngineConfig {
    pub models_dir: PathBuf,
    pub pdfium_library: Option<PathBuf>,
    pub dpi: f32,
    /// Long-side limit for text detection. Documents need a larger value than
    /// the scene-text default; see [`onnx::DETECT_MAX_SIDE`].
    pub det_max_side: u32,
}

impl Default for OcrEngineConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("models"),
            pdfium_library: None,
            dpi: 300.0,
            det_max_side: onnx::DETECT_MAX_SIDE,
        }
    }
}

/// The loaded OCR pipeline. `ModelSet` and `Pdfium` are both `Send + Sync`, so
/// page processing runs on rayon.
pub struct OcrEngine {
    models: Arc<ModelSet>,
    pdfium: Option<Arc<Pdfium>>,
    dpi: f32,
    det_max_side: u32,
}

/// The recognized structure of one page.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PageResult {
    pub page_index: usize,
    pub width: u32,
    pub height: u32,
    pub layout: PageLayout,
    /// Plain text in reading order, with a blank line between regions.
    pub text: String,
}

/// Progress events for the Tauri command to forward to the UI.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineProgress {
    Started { pages: usize },
    PageDone { page: usize, pages: usize },
    Finished,
}

/// Whether the caller asked this run to stop.
///
/// `None` is the non-cancellable path and never stops. The load is relaxed
/// ordering on a one-way flag: the only requirement is that the next page sees
/// the write, which an atomic store guarantees on every platform.
fn stop_requested(cancel: Option<&AtomicBool>) -> OcrResult<()> {
    match cancel {
        Some(flag) if flag.load(Ordering::Relaxed) => Err(OcrError::Cancelled),
        _ => Ok(()),
    }
}

/// Resolves a requested 1-based inclusive page window into a 0-based range.
///
/// Validation happens before any render so a caller gets one clear error
/// instead of a partial success. A first page past the end is a mistake worth
/// reporting; a last page past the end is clamped, because asking for "pages 1
/// to 999" of a ten-page file is a reasonable thing to do.
fn pdf_page_window(
    total: usize,
    range: Option<(usize, usize)>,
) -> OcrResult<std::ops::Range<usize>> {
    let Some((first, last)) = range else {
        return Ok(0..total);
    };
    if first == 0 || last == 0 {
        return Err(OcrError::Pdf("page numbers start at 1".to_string()));
    }
    if last < first {
        return Err(OcrError::Pdf(format!(
            "invalid page range: last page {last} is before first page {first}"
        )));
    }
    if total == 0 {
        return Err(OcrError::Pdf("the document has no pages".to_string()));
    }
    if first > total {
        return Err(OcrError::Pdf(format!(
            "page {first} is past the end of the document ({total} pages)"
        )));
    }
    Ok(first - 1..last.min(total))
}

/// How much real text a page's own layer must hold before OCR is skipped.
///
/// Counted in letters and digits, not bytes or lines: a scanned page with a
/// "Page 3" stamp has a text layer and still needs OCR for its body, so a byte
/// count would skip exactly the pages that need it most. One short line is
/// about 24 characters, the point below which OCR could still add something a
/// search would use.
const MIN_NATIVE_TEXT_CHARS: usize = 24;

/// The page's own text when it is rich enough to search, `None` when the page
/// still needs OCR.
fn usable_native_text(text: &str) -> Option<String> {
    let meaningful = text.chars().filter(|ch| ch.is_alphanumeric()).count();
    if meaningful < MIN_NATIVE_TEXT_CHARS {
        return None;
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// A page read from its own text layer, shaped like an OCR result so callers do
/// not need two code paths.
///
/// There are no per-line boxes to report - the PDF does not expose them through
/// this API - so the page carries one region covering it. `confidence` is 1.0
/// because the characters are the file's own, not a guess.
fn native_page_result(page_index: usize, native: pdf::NativePage, text: String) -> PageResult {
    PageResult {
        page_index,
        width: native.width,
        height: native.height,
        layout: PageLayout {
            width: native.width,
            height: native.height,
            regions: vec![Region {
                kind: RegionKind::Text,
                bbox: BBox::new(0.0, 0.0, native.width as f32, native.height as f32),
                confidence: 1.0,
                text: Some(text.clone()),
                table: None,
            }],
        },
        text,
    }
}

/// A rasterized page preview for the Studio's side-by-side view.
pub struct PreviewImage {
    pub png: Vec<u8>,
    /// Pixel dimensions of the full-resolution page, so region boxes can be
    /// scaled onto the (smaller) preview.
    pub width: u32,
    pub height: u32,
}

impl OcrEngine {
    pub fn new(config: OcrEngineConfig) -> OcrResult<Self> {
        let paths = ModelPaths::from_dir(&config.models_dir);
        Self::with_paths(paths, config)
    }

    /// Builds an engine from explicit model paths.
    ///
    /// The desktop shell uses this to load a downloaded model pack: a pack
    /// replaces detection and recognition while the layout and table models
    /// stay shared with the bundled folder, which no single directory can
    /// express. `config.models_dir` is ignored on this path.
    pub fn with_paths(paths: ModelPaths, config: OcrEngineConfig) -> OcrResult<Self> {
        let models = Arc::new(onnx::load_models(&paths)?);
        // One pdfium per process, shared by every engine this process builds.
        // See `pdf::shared_pdfium`: re-binding is what produced
        // `PdfiumLibraryBindingsAlreadyInitialized` after a model-pack switch.
        let pdfium = pdf::shared_pdfium(config.pdfium_library.as_deref())?;
        Ok(Self {
            models,
            pdfium,
            dpi: config.dpi,
            det_max_side: config.det_max_side,
        })
    }

    /// Processes a raster image (PNG/JPEG/TIFF/BMP/WebP).
    pub fn process_image(&self, path: &Path) -> OcrResult<PageResult> {
        // The camera's orientation tag is applied here, not by the decoder:
        // `image::open` ignores it and a portrait phone photo would arrive
        // sideways. The preview uses the same loader, so boxes line up.
        let image = crate::imageio::open_oriented(path)?;
        self.process_page_image(0, &image)
    }

    /// Renders a page (PDF or raster) as a downscaled PNG for the Studio preview.
    ///
    /// The Studio shows the page beside the extracted text and overlays region
    /// boxes, so it needs pixels rather than a filesystem path: a data URL behaves
    /// identically on every platform and needs no asset-protocol scope.
    pub fn page_preview(&self, path: &Path, page: usize, max_side: u32) -> OcrResult<PreviewImage> {
        let source = if has_extension(path, "pdf") {
            let pdfium = self.pdfium.as_ref().ok_or_else(|| {
                OcrError::Pdf(
                    "no pdfium library configured; set ZEIDEX_PDFIUM_LIBRARY or bundle the shared library"
                        .to_string(),
                )
            })?;
            let document = pdfium
                .load_pdf_from_file(path, None)
                .map_err(|err| OcrError::Pdf(err.to_string()))?;
            pdf::render_document_page(&document, page as i32, self.dpi)?
        } else {
            // Same loader as `process_image`: a preview drawn from the raw
            // pixels of a rotated photo would not match the recognized boxes.
            crate::imageio::open_oriented(path)?
        };

        let (width, height) = source.dimensions();
        let max_side = max_side.max(64);
        let preview = if width.max(height) > max_side {
            source.resize(max_side, max_side, image::imageops::FilterType::Triangle)
        } else {
            source
        };
        let mut png = Vec::new();
        preview.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        Ok(PreviewImage { png, width, height })
    }

    /// Renders every PDF page, then OCRs the pages in parallel.
    ///
    /// Rendering stays sequential because pdfium documents are not guaranteed to
    /// be thread-safe; the expensive part (detection + recognition) is what rayon
    /// parallelizes. Pages are processed in bounded chunks so a long document does
    /// not hold every 300 DPI bitmap in memory at once.
    pub fn process_pdf(
        &self,
        path: &Path,
        progress: &(dyn Fn(PipelineProgress) + Sync),
    ) -> OcrResult<Vec<PageResult>> {
        self.process_pdf_window(path, None, None, progress)
    }

    /// Same as [`Self::process_pdf`], but stops between pages when `cancel` is
    /// set. A page already being recognized finishes first: ONNX Runtime has no
    /// interruption point, so the worst case is one page, not the document.
    pub fn process_pdf_cancellable(
        &self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &(dyn Fn(PipelineProgress) + Sync),
    ) -> OcrResult<Vec<PageResult>> {
        self.process_pdf_window(path, None, Some(cancel), progress)
    }

    /// Renders pages `first..=last` (1-based) and OCRs only those pages.
    ///
    /// The Studio uses this for a long document: a 200-page scan rarely needs
    /// every page, and a range keeps the memory and time proportional to what
    /// the user asked for. `PageResult::page_index` stays the document's own
    /// index, so a page keeps its identity after a partial run.
    pub fn process_pdf_range(
        &self,
        path: &Path,
        first: usize,
        last: usize,
        progress: &(dyn Fn(PipelineProgress) + Sync),
    ) -> OcrResult<Vec<PageResult>> {
        self.process_pdf_window(path, Some((first, last)), None, progress)
    }

    /// The same window with a cancellation flag, for the shell's Cancel button.
    pub fn process_pdf_range_cancellable(
        &self,
        path: &Path,
        first: usize,
        last: usize,
        cancel: &AtomicBool,
        progress: &(dyn Fn(PipelineProgress) + Sync),
    ) -> OcrResult<Vec<PageResult>> {
        self.process_pdf_window(path, Some((first, last)), Some(cancel), progress)
    }

    fn process_pdf_window(
        &self,
        path: &Path,
        range: Option<(usize, usize)>,
        cancel: Option<&AtomicBool>,
        progress: &(dyn Fn(PipelineProgress) + Sync),
    ) -> OcrResult<Vec<PageResult>> {
        // A run cancelled before it starts must not open the document, emit
        // progress, or spend a page's worth of CPU to learn it was cancelled.
        stop_requested(cancel)?;
        let pdfium = self.pdfium.as_ref().ok_or_else(|| {
            OcrError::Pdf(
                "no pdfium library configured; set ZEIDEX_PDFIUM_LIBRARY or bundle the shared library"
                    .to_string(),
            )
        })?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|err| OcrError::Pdf(err.to_string()))?;
        let total = pdf::page_count(&document).max(0) as usize;
        let window = pdf_page_window(total, range)?;
        let selected = window.len();

        progress(PipelineProgress::Started { pages: selected });
        let chunk = rayon::current_num_threads().max(1);
        let mut results: Vec<PageResult> = Vec::with_capacity(selected);
        let mut start = window.start;
        while start < window.end {
            let end = (start + chunk).min(window.end);
            let mut slots: Vec<Option<PageResult>> = (start..end).map(|_| None).collect();
            let mut images: Vec<(usize, DynamicImage)> = Vec::new();

            // Deciding per page, before rendering: a page whose own text layer
            // is enough never becomes a 300 DPI bitmap, which is the difference
            // between a born-digital PDF costing a second and costing a minute.
            for index in start..end {
                stop_requested(cancel)?;
                let native = pdf::native_page_text(&document, index as i32, self.dpi)?;
                match usable_native_text(&native.text) {
                    Some(text) => {
                        slots[index - start] = Some(native_page_result(index, native, text));
                        progress(PipelineProgress::PageDone {
                            page: index - window.start,
                            pages: selected,
                        });
                    }
                    None => images.push((
                        index,
                        pdf::render_document_page(&document, index as i32, self.dpi)?,
                    )),
                }
            }

            let chunk_results: Vec<(usize, OcrResult<PageResult>)> = images
                .par_iter()
                .map(|(index, image)| {
                    let result = stop_requested(cancel)
                        .and_then(|()| self.process_page_image(*index, image));
                    // Progress counts work done within this run; the page a
                    // result belongs to is `PageResult::page_index`. A range
                    // therefore reports "3 of 5 selected", never "page 42 of 5".
                    progress(PipelineProgress::PageDone {
                        page: index - window.start,
                        pages: selected,
                    });
                    (*index, result)
                })
                .collect();
            for (index, result) in chunk_results {
                slots[index - start] = Some(result?);
            }
            for (offset, slot) in slots.into_iter().enumerate() {
                let page = slot.ok_or_else(|| {
                    OcrError::Pdf(format!(
                        "page {} produced neither native text nor OCR output",
                        start + offset
                    ))
                })?;
                results.push(page);
            }
            start = end;
        }
        progress(PipelineProgress::Finished);
        Ok(results)
    }

    fn process_page_image(&self, page_index: usize, image: &DynamicImage) -> OcrResult<PageResult> {
        let (width, height) = image.dimensions();

        // A layout failure is a real error: silently degrading to a single
        // full-page text region would hide a broken or mismatched layout model.
        let page_layout = onnx::detect_layout(&self.models, image)?;

        let lines = onnx::detect_text_lines_with_limit(&self.models, image, self.det_max_side)?;
        let mut recognized: Vec<(BBox, String)> = Vec::with_capacity(lines.len());
        for line in &lines {
            let (text, _confidence) = onnx::recognize_line(&self.models, image, *line)?;
            if !text.trim().is_empty() {
                recognized.push((line.bbox, bidi::normalize_logical(&text)));
            }
        }

        let mut regions = page_layout.regions.clone();
        for region in regions.iter_mut() {
            match region.kind {
                RegionKind::Table => {
                    if let Ok(mut structure) = onnx::detect_table(&self.models, image, region) {
                        table::assign_texts(&mut structure, &recognized);
                        region.table = Some(structure);
                    }
                }
                _ => {
                    let mut lines_in_region: Vec<&(BBox, String)> = recognized
                        .iter()
                        .filter(|(bbox, _)| region.bbox.contains(bbox.center()))
                        .collect();
                    lines_in_region.sort_by(|a, b| {
                        a.0.y0
                            .partial_cmp(&b.0.y0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(
                                a.0.x0
                                    .partial_cmp(&b.0.x0)
                                    .unwrap_or(std::cmp::Ordering::Equal),
                            )
                    });
                    let text = lines_in_region
                        .into_iter()
                        .map(|(_, text)| text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    region.text = Some(text);

                    // The layout model has five classes; a table it labels
                    // `figure` or `text` would otherwise reach the exporters as
                    // flat text. Ruling lines recover the grid when the region
                    // really contains one, and the empty-cell guard keeps a
                    // framed photo from becoming a table.
                    if let Some(mut structure) = table::from_ruling_lines(image, region.bbox) {
                        table::assign_texts(&mut structure, &recognized);
                        if structure
                            .cells
                            .iter()
                            .any(|cell| !cell.text.trim().is_empty())
                        {
                            region.kind = RegionKind::Table;
                            region.text = None;
                            region.table = Some(structure);
                        }
                    }
                }
            }
        }

        // An Arabic page reads right-to-left. The layout model emits regions
        // left-to-right, so the order is corrected once the text is known — before
        // the flat page text is flattened from it.
        let rtl = regions.iter().any(|region| {
            region.text.as_deref().is_some_and(bidi::is_rtl)
                || region.table.as_ref().is_some_and(|structure| {
                    structure.cells.iter().any(|cell| bidi::is_rtl(&cell.text))
                })
        });
        if rtl {
            layout::sort_reading_order(&mut regions, true);
        }

        let mut text = String::new();
        for region in &regions {
            match region.kind {
                RegionKind::Table => {
                    if let Some(structure) = &region.table {
                        for cell in &structure.cells {
                            if !cell.text.is_empty() {
                                text.push_str(&cell.text);
                                text.push('\t');
                            }
                        }
                        text.push('\n');
                    }
                }
                _ => {
                    if let Some(region_text) = &region.text
                        && !region_text.is_empty()
                    {
                        text.push_str(region_text);
                        text.push_str("\n\n");
                    }
                }
            }
        }

        Ok(PageResult {
            page_index,
            width,
            height,
            layout: PageLayout {
                width,
                height,
                regions,
            },
            text,
        })
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests {
    use super::{pdf_page_window, stop_requested, usable_native_text};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn a_run_without_a_flag_never_stops() {
        assert!(stop_requested(None).is_ok());
    }

    #[test]
    fn a_set_flag_stops_the_run_with_the_cancelled_error() {
        let flag = AtomicBool::new(false);
        assert!(stop_requested(Some(&flag)).is_ok());
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let err = stop_requested(Some(&flag)).expect_err("must stop");
        assert_eq!(err.to_string(), "cancelled");
    }

    #[test]
    fn all_pages_when_no_window_is_asked_for() {
        assert_eq!(pdf_page_window(5, None).ok(), Some(0..5));
        assert_eq!(pdf_page_window(0, None).ok(), Some(0..0));
    }

    #[test]
    fn an_inclusive_window_becomes_a_zero_based_range() {
        assert_eq!(pdf_page_window(10, Some((3, 5))).ok(), Some(2..5));
        assert_eq!(pdf_page_window(10, Some((1, 1))).ok(), Some(0..1));
        assert_eq!(pdf_page_window(10, Some((10, 10))).ok(), Some(9..10));
    }

    #[test]
    fn a_last_page_past_the_end_is_clamped() {
        assert_eq!(pdf_page_window(4, Some((2, 99))).ok(), Some(1..4));
    }

    #[test]
    fn a_first_page_past_the_end_is_an_error() {
        let err = pdf_page_window(4, Some((5, 6))).expect_err("must reject");
        assert!(err.to_string().contains("past the end"), "{err}");
    }

    #[test]
    fn a_reversed_zero_based_or_empty_window_is_an_error() {
        assert!(pdf_page_window(4, Some((3, 2))).is_err());
        assert!(pdf_page_window(4, Some((0, 1))).is_err());
        assert!(pdf_page_window(0, Some((1, 1))).is_err());
    }

    #[test]
    fn a_page_stamp_is_not_enough_text_to_skip_ocr() {
        // A scanned page with this stamp still has a body that only OCR can
        // read; the threshold counts letters, so "Page 3" stays below it.
        assert!(usable_native_text("Page 3").is_none());
        assert!(usable_native_text("   \n\t ").is_none());
    }

    #[test]
    fn a_page_of_prose_skips_ocr_and_is_trimmed() {
        let text = "This page carries a real text layer, so OCR must not run on it.\n";
        let usable = usable_native_text(text).expect("prose must qualify");
        assert_eq!(
            usable,
            "This page carries a real text layer, so OCR must not run on it."
        );
        // Arabic letters count too, and folding is not this layer's job.
        assert!(usable_native_text("هذا نص عربي كافٍ لتجاوز التعرف الضوئي").is_some());
    }
}
