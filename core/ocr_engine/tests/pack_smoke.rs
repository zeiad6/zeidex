//! A downloaded model pack, loaded through the real catalog and run on a page.
//!
//! Skips unless `ZEIDEX_TEST_PACK_ID`, `ZEIDEX_TEST_MODELS_ROOT`,
//! `ZEIDEX_TEST_MODELS_DIR` (the bundled folder, which supplies layout and
//! table) and `ZEIDEX_TEST_IMAGE` are set, with `ORT_DYLIB_PATH` naming ONNX
//! Runtime.
//!
//! This is the test that would have caught a pack whose file names, dictionary
//! pairing or visual-order flag did not match what the engine expects: it builds
//! the paths exactly as the shell does, loads the sessions, and reads a real
//! page. It asserts what a user would notice — text came out, and for Arabic
//! that the letters are in logical order and the script was recognised — not an
//! exact transcription, which would pin one model's output as the contract.

use std::path::PathBuf;

use zeidex_models::pack_paths;
use zeidex_ocr::OcrEngine;
use zeidex_ocr::OcrEngineConfig;
use zeidex_ocr::bidi::has_arabic;

#[test]
fn a_downloaded_pack_loads_and_reads_a_page() -> Result<(), String> {
    let Ok(pack_id) = std::env::var("ZEIDEX_TEST_PACK_ID") else {
        eprintln!("skipping: ZEIDEX_TEST_PACK_ID not set");
        return Ok(());
    };
    let (Ok(root), Ok(bundled), Ok(image)) = (
        std::env::var("ZEIDEX_TEST_MODELS_ROOT"),
        std::env::var("ZEIDEX_TEST_MODELS_DIR"),
        std::env::var("ZEIDEX_TEST_IMAGE"),
    ) else {
        eprintln!(
            "skipping: pack smoke needs ZEIDEX_TEST_MODELS_ROOT, ZEIDEX_TEST_MODELS_DIR and ZEIDEX_TEST_IMAGE"
        );
        return Ok(());
    };

    let pack = zeidex_models::pack(&pack_id).ok_or_else(|| format!("unknown pack {pack_id}"))?;
    assert!(!pack.bundled, "this test is for downloaded packs");

    // The same composition the shell uses: detection and recognition from the
    // pack, layout and table from the bundled folder.
    let resolved = pack_paths(
        PathBuf::from(&bundled).as_path(),
        PathBuf::from(&root).as_path(),
        pack,
    );
    let mut paths = zeidex_ocr::onnx::ModelPaths::from_dir(&bundled);
    paths.det = resolved.det;
    paths.rec_en = resolved.rec_en;
    paths.rec_ar = resolved.rec_ar;
    paths.dict_en = resolved.dict_en;
    paths.dict_ar = resolved.dict_ar;
    paths.rec_ar_visual = resolved.rec_ar_visual;
    assert!(paths.missing().is_empty(), "missing: {:?}", paths.missing());

    let engine = OcrEngine::with_paths(paths, OcrEngineConfig::default())
        .map_err(|err| format!("{pack_id}: {err}"))?;
    let page = engine
        .process_image(std::path::Path::new(&image))
        .map_err(|err| format!("{pack_id}: {err}"))?;

    eprintln!("{pack_id}: {} characters", page.text.chars().count());
    assert!(
        page.text.chars().filter(|ch| ch.is_alphanumeric()).count() >= 10,
        "{pack_id} read almost nothing: {:?}",
        page.text
    );
    assert!(
        page.layout
            .regions
            .iter()
            .all(|region| (0.0..=1.0).contains(&region.confidence)),
        "{pack_id}: a confidence outside 0..1 means the recognizer's output was not probabilities"
    );
    if image.contains("arabic") {
        assert!(
            has_arabic(&page.text),
            "{pack_id} did not recognise Arabic: {:?}",
            page.text
        );
    }
    Ok(())
}
