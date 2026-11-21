use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;

use crate::bidi;
use crate::error::{OcrError, OcrResult};
use crate::geometry::BBox;
use crate::layout::{self, PageLayout, RegionKind};
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
        let models = Arc::new(onnx::load_models(&paths)?);
        let pdfium = match &config.pdfium_library {
            Some(path) => Some(Arc::new(pdf::bind_pdfium(Some(path))?)),
            None => None,
        };
        Ok(Self {
            models,
            pdfium,
            dpi: config.dpi,
            det_max_side: config.det_max_side,
        })
    }

    /// Processes a raster image (PNG/JPEG/TIFF/BMP/WebP).
    pub fn process_image(&self, path: &Path) -> OcrResult<PageResult> {
        let image = image::open(path)?;
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
            image::open(path)?
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

        progress(PipelineProgress::Started { pages: total });
        let chunk = rayon::current_num_threads().max(1);
        let mut results: Vec<PageResult> = Vec::with_capacity(total);
        let mut start = 0usize;
        while start < total {
            let end = (start + chunk).min(total);
            let images = (start..end)
                .map(|index| pdf::render_document_page(&document, index as i32, self.dpi))
                .collect::<OcrResult<Vec<_>>>()?;
            let chunk_results: Vec<OcrResult<PageResult>> = images
                .par_iter()
                .enumerate()
                .map(|(offset, image)| {
                    let page = start + offset;
                    let result = self.process_page_image(page, image);
                    progress(PipelineProgress::PageDone { page, pages: total });
                    result
                })
                .collect();
            for result in chunk_results {
                results.push(result?);
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
