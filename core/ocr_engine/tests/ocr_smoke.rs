//! Real-model smoke tests for detection, recognition and table structure.
//! Skips unless `ZEIDEX_TEST_MODELS_DIR` and `ZEIDEX_TEST_IMAGE` are set and
//! `ORT_DYLIB_PATH` names an ONNX Runtime shared library.

use zeidex_ocr::error::OcrResult;
use zeidex_ocr::layout::RegionKind;
use zeidex_ocr::onnx::{
    ModelPaths, detect_layout, detect_table, detect_text_lines, load_models, recognize_line,
};

#[test]
fn ocr_pipeline_runs_on_a_real_page() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_IMAGE not set");
        return Ok(());
    };

    let paths = ModelPaths::from_dir(&models_dir);
    let models = load_models(&paths)?;
    let image = image::open(&image_path).map_err(zeidex_ocr::OcrError::Image)?;

    let lines = detect_text_lines(&models, &image)?;
    eprintln!("detected {} text lines", lines.len());
    for line in lines.iter().take(5) {
        eprintln!("  line {:?}", line.bbox);
    }
    assert!(!lines.is_empty(), "text detection found nothing");

    let mut recognized = Vec::new();
    for line in lines.iter().take(4) {
        let (text, confidence) = recognize_line(&models, &image, *line)?;
        eprintln!("  rec {:.3} {:?}", confidence, text);
        recognized.push(text);
    }
    assert!(
        recognized.iter().any(|text| !text.trim().is_empty()),
        "recognition produced no text"
    );

    let layout = detect_layout(&models, &image)?;
    let table = layout
        .regions
        .iter()
        .find(|region| region.kind == RegionKind::Table)
        .expect("layout has a table region");
    let structure = detect_table(&models, &image, table)?;
    eprintln!(
        "table {}x{} with {} cells",
        structure.rows,
        structure.cols,
        structure.cells.len()
    );
    assert!(structure.rows > 1, "SLANet found no rows");
    Ok(())
}
