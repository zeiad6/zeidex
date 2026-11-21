//! Real-model smoke tests. They skip unless `ZEIDEX_TEST_MODELS_DIR` points at a
//! downloaded model set and `ORT_DYLIB_PATH` names an ONNX Runtime shared
//! library, so CI stays offline and fast.

use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_layout, load_models};

#[test]
fn pico_layout_decodes_a_real_page() -> OcrResult<()> {
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
    let layout = detect_layout(&models, &image)?;

    for region in &layout.regions {
        eprintln!(
            "{:?} {:.3} {:?}",
            region.kind, region.confidence, region.bbox
        );
    }
    assert!(!layout.regions.is_empty(), "layout returned no regions");
    Ok(())
}
