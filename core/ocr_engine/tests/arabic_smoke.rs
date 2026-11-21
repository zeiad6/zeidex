//! Arabic recognition on a fixed, print-like fixture.
//!
//! Skips unless `ZEIDEX_TEST_MODELS_DIR` and `ZEIDEX_TEST_ARABIC_IMAGE` are set
//! and `ORT_DYLIB_PATH` names an ONNX Runtime shared library.
//!
//! The fixture (`fixtures/arabic_lines.png`) is three lines rendered with
//! Pillow, Noto Naskh Arabic Bold at 44 px, 60 px line pitch, `direction="rtl"`,
//! `language="ar"` on white — the same kind of clean print a scan produces.
//!
//! Two short lines are read exactly by `arabic_PP-OCRv4_rec`; the test pins
//! that result so a dictionary, preprocessing or model change that breaks
//! Arabic cannot pass unnoticed. The third line is deliberately longer than the
//! recognizer handles well: only its true prefix and suffix are asserted, so
//! the test records the limitation without blessing the error.

use zeidex_ocr::bidi::has_arabic;
use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_text_lines, load_models, recognize_line};

#[test]
fn arabic_lines_are_recognized() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_ARABIC_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_ARABIC_IMAGE not set");
        return Ok(());
    };

    let models = load_models(&ModelPaths::from_dir(&models_dir))?;
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

    let long = recognized
        .iter()
        .find(|(text, _)| text.starts_with("السلام") && text.ends_with("الله"))
        .expect("the long line keeps its first and last word");
    assert!(
        long.0.len() > "السلام عليكم ورحمة الله".len() / 2,
        "the long line should still produce most of its text: {:?}",
        long.0
    );

    Ok(())
}
