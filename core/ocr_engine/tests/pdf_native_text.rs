//! A PDF that already carries text must be read, not re-recognized.
//!
//! `fixtures/text_layer.pdf` is a one-page, born-digital PDF written by
//! `tools/make-text-pdf.py`: two lines of Helvetica with no raster content.
//! Reading it through the pipeline must return those exact characters, which
//! proves pdfium's text API was used - OCR of a 300 DPI render would be slower
//! and would not reproduce them exactly.
//!
//! The page-shape assertion is the second half of the proof: the native path
//! reports one full-page region, while the OCR path reports the line boxes the
//! detector found.

use std::path::{Path, PathBuf};

use zeidex_ocr::error::OcrResult;
use zeidex_ocr::pipeline::{OcrEngine, OcrEngineConfig};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn pdfium_library() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("ZEIDEX_TEST_PDFIUM_LIB").ok()?);
    path.is_file().then_some(path)
}

fn models_dir() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("ZEIDEX_TEST_MODELS_DIR").ok()?);
    path.is_dir().then_some(path)
}

#[test]
fn a_born_digital_pdf_is_read_without_running_ocr() -> OcrResult<()> {
    let (Some(models_dir), Some(pdfium_library)) = (models_dir(), pdfium_library()) else {
        eprintln!("skipping: set ZEIDEX_TEST_MODELS_DIR and ZEIDEX_TEST_PDFIUM_LIB");
        return Ok(());
    };

    let engine = OcrEngine::new(OcrEngineConfig {
        models_dir,
        pdfium_library: Some(pdfium_library),
        dpi: 300.0,
        det_max_side: zeidex_ocr::onnx::DETECT_MAX_SIDE,
    })?;

    let pages = engine.process_pdf(&fixture("text_layer.pdf"), &|_| {})?;
    assert_eq!(pages.len(), 1, "the fixture has one page");
    let page = &pages[0];

    assert!(
        page.text.contains("ZEIDEX-NATIVE-TEXT-42"),
        "native text was not used: {:?}",
        page.text
    );
    assert!(
        page.text
            .contains("real text layer, so OCR must not run on it"),
        "native text lost its second line: {:?}",
        page.text
    );

    // A4 at 300 DPI, computed from the page's points without rendering it.
    assert_eq!((page.width, page.height), (2479, 3508));

    assert_eq!(
        page.layout.regions.len(),
        1,
        "the native path reports one page-sized region, not detector boxes"
    );
    let region = &page.layout.regions[0];
    assert_eq!(region.bbox.x0, 0.0);
    assert_eq!(region.bbox.y0, 0.0);
    assert_eq!(region.bbox.x1, page.width as f32);
    assert_eq!(region.bbox.y1, page.height as f32);
    assert_eq!(region.confidence, 1.0);
    assert_eq!(region.text.as_deref(), Some(page.text.as_str()));

    Ok(())
}
