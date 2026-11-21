//! Real-model PDF test.
//!
//! The raster path is covered by `ocr_smoke`, `table_smoke` and `ocr_accuracy`.
//! This test covers the other half of the shipped flow: pdfium opens the file,
//! renders page 0 at 300 dpi, the same pipeline OCRs the bitmap, and the Studio
//! preview can rasterize that page. It skips unless the models, the fixture and
//! a pdfium shared library are all provided.
//!
//! `fixtures/table.pdf` is the table fixture placed on an A4 page at exactly
//! 300 dpi (288x192 pt), so pdfium's 300 dpi render reproduces the original
//! table pixels 1:1 and the test measures the PDF plumbing rather than
//! resampling quality.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::GenericImageView;

use zeidex_ocr::RegionKind;
use zeidex_ocr::pipeline::{OcrEngine, OcrEngineConfig, PipelineProgress};

#[test]
fn pdf_renders_and_ocrs_into_a_real_table() -> zeidex_ocr::OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(pdf_path) = std::env::var("ZEIDEX_TEST_PDF") else {
        eprintln!("skipping: ZEIDEX_TEST_PDF not set");
        return Ok(());
    };
    let Ok(pdfium_lib) = std::env::var("ZEIDEX_TEST_PDFIUM_LIB") else {
        eprintln!("skipping: ZEIDEX_TEST_PDFIUM_LIB not set");
        return Ok(());
    };
    if !Path::new(&pdfium_lib).is_file() {
        eprintln!("skipping: pdfium library is missing");
        return Ok(());
    }

    let engine = OcrEngine::new(OcrEngineConfig {
        models_dir: models_dir.into(),
        pdfium_library: Some(pdfium_lib.into()),
        ..OcrEngineConfig::default()
    })?;

    let done = AtomicUsize::new(0);
    let started = AtomicUsize::new(0);
    let progress = |update: PipelineProgress| match update {
        PipelineProgress::Started { pages } => {
            started.store(pages, Ordering::SeqCst);
        }
        PipelineProgress::PageDone { .. } => {
            done.fetch_add(1, Ordering::SeqCst);
        }
        PipelineProgress::Finished => {}
    };
    let pages = engine.process_pdf(Path::new(&pdf_path), &progress)?;

    assert_eq!(started.load(Ordering::SeqCst), 1, "fixture is one page");
    assert_eq!(pages.len(), 1, "every page must be OCR'd");
    assert_eq!(done.load(Ordering::SeqCst), 1, "one PageDone event");
    let page = &pages[0];
    assert_eq!(page.page_index, 0);
    assert!(
        page.text.contains("Quantity") && page.text.contains("Widget A"),
        "page text lost the table: {:?}",
        page.text
    );

    let table = page
        .layout
        .regions
        .iter()
        .find(|region| region.kind == RegionKind::Table)
        .expect("layout has a table region on the rendered PDF page");
    let structure = table.table.as_ref().expect("table region has a grid");
    assert_eq!(
        (structure.rows, structure.cols, structure.cells.len()),
        (4, 3, 12),
        "PDF table grid: {}x{} with {} cells",
        structure.rows,
        structure.cols,
        structure.cells.len()
    );
    let mut texts: Vec<&str> = structure
        .cells
        .iter()
        .map(|cell| cell.text.trim())
        .collect();
    texts.sort_unstable();
    let mut expected = [
        "12", "129.00", "19.99", "3", "4.50", "7", "Gadget C", "Item", "Price", "Quantity",
        "Widget A", "Widget B",
    ];
    expected.sort_unstable();
    assert_eq!(texts, expected, "each PDF cell carries its recognized text");

    let preview = engine.page_preview(Path::new(&pdf_path), 0, 1200)?;
    let image = image::load_from_memory(&preview.png).expect("preview is a PNG");
    let (width, height) = image.dimensions();
    assert!(width > 0 && height > 0, "preview has no pixels");
    assert!(
        width.max(height) <= 1200,
        "preview ignored max_side: {width}x{height}"
    );
    // A4 at 300 dpi is 2480x3508; allow a pixel of rounding either way.
    assert!(
        preview.width.abs_diff(2480) <= 2 && preview.height.abs_diff(3508) <= 2,
        "A4 at 300 dpi, got {}x{}",
        preview.width,
        preview.height
    );

    Ok(())
}
