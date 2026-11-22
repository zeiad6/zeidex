//! PDFium lifecycle regression tests.
//!
//! `pdfium-render` allows exactly one set of library bindings per process:
//! `Pdfium::bind_to_library` returns `PdfiumLibraryBindingsAlreadyInitialized`
//! the second time it is called. Building a second `OcrEngine` in one process is
//! normal for Zeidex - activating a downloaded model pack resets the cached
//! engine, and the benchmark builds a throwaway engine beside the live one - so
//! a second engine used to fail with that message and take image OCR down with
//! it, because the engine load failed before any recognition could start.
//!
//! These tests build engines in the same process, in the same way the shell
//! does, and require both of them to open and render PDFs.
//!
//! They skip unless the models, the fixture and a pdfium shared library are all
//! provided, which is what `make ocr-test` does.

use std::path::Path;
use std::sync::Arc;

use zeidex_ocr::OcrEngine;
use zeidex_ocr::OcrEngineConfig;
use zeidex_ocr::pdf;

struct Fixtures {
    models_dir: String,
    pdf: String,
    pdfium: String,
}

fn fixtures() -> Option<Fixtures> {
    let models_dir = std::env::var("ZEIDEX_TEST_MODELS_DIR").ok()?;
    let pdf = std::env::var("ZEIDEX_TEST_PDF").ok()?;
    let pdfium = std::env::var("ZEIDEX_TEST_PDFIUM_LIB").ok()?;
    if !Path::new(&pdfium).is_file() || !Path::new(&pdf).is_file() {
        return None;
    }
    Some(Fixtures {
        models_dir,
        pdf,
        pdfium,
    })
}

fn engine(fixtures: &Fixtures) -> zeidex_ocr::OcrResult<OcrEngine> {
    OcrEngine::new(OcrEngineConfig {
        models_dir: fixtures.models_dir.clone().into(),
        pdfium_library: Some(fixtures.pdfium.clone().into()),
        ..OcrEngineConfig::default()
    })
}

#[test]
fn a_second_engine_reuses_the_bound_pdfium() -> zeidex_ocr::OcrResult<()> {
    let Some(fixtures) = fixtures() else {
        eprintln!(
            "skipping: set ZEIDEX_TEST_MODELS_DIR, ZEIDEX_TEST_PDF and ZEIDEX_TEST_PDFIUM_LIB"
        );
        return Ok(());
    };
    let pdf = Path::new(&fixtures.pdf);
    let no_progress = |_event: zeidex_ocr::PipelineProgress| {};

    // The live engine: open, OCR, and render a preview.
    let first = engine(&fixtures)?;
    let pages = first.process_pdf(pdf, &no_progress)?;
    assert_eq!(pages.len(), 1, "the fixture is one page");
    assert!(
        pages[0].text.contains("Widget A"),
        "the first run must recognize the page: {:?}",
        pages[0].text
    );
    let preview = first.page_preview(pdf, 0, 800)?;
    assert!(
        !preview.png.is_empty(),
        "the first engine must render a preview"
    );
    drop(first);

    // This construction is the regression: it used to return
    // `PdfiumLibraryBindingsAlreadyInitialized` and fail the whole engine.
    let second = engine(&fixtures)?;
    let again = second.process_pdf(pdf, &no_progress)?;
    assert_eq!(
        again.len(),
        1,
        "a second engine must still open the same PDF"
    );
    assert!(
        again[0].text.contains("Widget A"),
        "the second engine must recognize the page: {:?}",
        again[0].text
    );

    // Repeated processing on the second engine, then a page window.
    let third = second.process_pdf(pdf, &no_progress)?;
    assert_eq!(third.len(), 1);
    let window = second.process_pdf_range(pdf, 1, 1, &no_progress)?;
    assert_eq!(window.len(), 1);
    assert_eq!(window[0].page_index, 0);

    // The handle itself is shared: two bind calls return the same allocation.
    let a = pdf::shared_pdfium(Some(Path::new(&fixtures.pdfium)))?.expect("pdfium is available");
    let b = pdf::shared_pdfium(Some(Path::new(&fixtures.pdfium)))?.expect("pdfium is available");
    assert!(
        Arc::ptr_eq(&a, &b),
        "every caller must share one bound pdfium, not bind again"
    );

    Ok(())
}
