//! Arabic recognition with the PP-OCRv5 mobile recognizer.
//!
//! Skips unless `ZEIDEX_TEST_V5_MODELS_DIR` is set to a model folder whose
//! `ppocrv4_rec_ar.onnx` is `arabic_PP-OCRv5_mobile_rec_onnx` (its extracted
//! dictionary in `arabic_dict.txt`), with the usual v4 detection, layout and
//! table files beside it, and `ORT_DYLIB_PATH` naming ONNX Runtime.
//!
//! This test exists because the v5 recognizer differs from v4 in a way the
//! code has to know about: it emits glyphs in **visual** order. The same
//! fixture that v4 reads in logical order comes out reversed, so the model is
//! loaded with `with_visual_arabic()` and the conversion is what is under test.
//! It pins the same two short lines as the v4 test, plus the long line that v4
//! can only partially read — v5 reads all of it.

use zeidex_ocr::bidi::has_arabic;
use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_text_lines, load_models, recognize_line};

#[test]
fn arabic_v5_lines_are_recognized() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_V5_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_V5_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_ARABIC_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_ARABIC_IMAGE not set");
        return Ok(());
    };

    let paths = ModelPaths::from_dir(&models_dir).with_visual_arabic();
    let models = load_models(&paths)?;
    let image = image::open(&image_path).map_err(zeidex_ocr::OcrError::Image)?;

    let lines = detect_text_lines(&models, &image)?;
    assert_eq!(lines.len(), 3, "fixture has three text lines");

    let mut recognized = Vec::new();
    for line in &lines {
        let (text, confidence) = recognize_line(&models, &image, *line)?;
        eprintln!("{:.3} {text:?}", confidence);
        assert!(has_arabic(&text), "recognized line is not Arabic: {text:?}");
        recognized.push((text, confidence));
    }

    let find = |expected: &str| {
        recognized
            .iter()
            .find(|(text, _)| text == expected)
            .map(|(text, confidence)| (text, *confidence))
    };

    let (_, confidence) =
        find("مرحبا بكم في لوميو").expect("short Arabic line should be read exactly");
    assert!(confidence > 0.6, "confidence {confidence} is too low");

    let (_, confidence) = find("كتاب الطالب").expect("short Arabic line should be read exactly");
    assert!(confidence > 0.6, "confidence {confidence} is too low");

    let (_, confidence) =
        find("السلام عليكم ورحمة الله").expect("v5 reads the long line in full, which v4 cannot");

    assert!(confidence > 0.6, "confidence {confidence} is too low");

    Ok(())
}
