//! A phone photo of a page, stored sideways with an EXIF orientation tag.
//!
//! Skips unless `ZEIDEX_TEST_MODELS_DIR` and `ZEIDEX_TEST_PHOTO` are set and
//! `ORT_DYLIB_PATH` names an ONNX Runtime shared library.
//!
//! The fixture (`fixtures/photo-table.jpg`) is `table.png` rotated 90° CCW and
//! saved with `Orientation=6` — the sensor frame a phone writes, which viewers
//! turn upright from the tag alone. The pipeline must apply that tag before
//! detection: without it the page is read sideways and comes back empty, and
//! the preview would disagree with the boxes drawn over it.

use std::path::Path;

use zeidex_ocr::error::OcrResult;
use zeidex_ocr::{OcrEngine, OcrEngineConfig};

#[test]
fn a_sideways_phone_photo_is_read_upright() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(photo_path) = std::env::var("ZEIDEX_TEST_PHOTO") else {
        eprintln!("skipping: ZEIDEX_TEST_PHOTO not set");
        return Ok(());
    };
    let pdfium = std::env::var("ZEIDEX_TEST_PDFIUM_LIB").ok();

    let engine = OcrEngine::new(OcrEngineConfig {
        models_dir: models_dir.into(),
        pdfium_library: pdfium.map(Into::into),
        ..OcrEngineConfig::default()
    })?;

    let page = engine.process_image(Path::new(&photo_path))?;

    // `table.png` is 1200×760. The stored JPEG is 760×1200; only an applied
    // orientation tag explains the upright frame.
    assert_eq!((page.width, page.height), (1200, 760));

    let text = page.text.to_lowercase();
    for expected in ["quantity", "widget", "129.00"] {
        assert!(
            text.contains(expected),
            "upright page should contain {expected:?}, got: {}",
            page.text
        );
    }

    // The preview reports the same frame, so region boxes land on the pixels
    // the user sees instead of on the sideways sensor frame.
    let preview = engine.page_preview(Path::new(&photo_path), 0, 800)?;
    assert_eq!((preview.width, preview.height), (1200, 760));
    assert!(preview.png.starts_with(b"\x89PNG"));

    Ok(())
}
