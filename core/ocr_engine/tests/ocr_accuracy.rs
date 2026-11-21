//! Recognition accuracy on the synthetic table fixture.
//!
//! Every cell's text is known, so this turns "the pipeline ran" into a number:
//! how many of the twelve cells come back exactly right. It is deliberately
//! narrow — one font, one clean render — and skips unless the same real-model
//! environment as `ocr_smoke` is present.

use std::path::Path;
use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_text_lines, load_models, recognize_line};

const EXPECTED: [&str; 12] = [
    "item", "quantity", "price", "widget a", "12", "19.99", "widget b", "7", "4.50", "gadget c",
    "3", "129.00",
];

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = if ca == cb {
                previous
            } else {
                1 + previous.min(row[j]).min(current)
            };
            previous = current;
        }
    }
    row[b.len()]
}

#[test]
fn recognizes_every_cell_of_the_table_fixture() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_IMAGE not set");
        return Ok(());
    };
    if Path::new(&image_path)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("table.png")
    {
        eprintln!("skipping: fixture is not table.png");
        return Ok(());
    }

    let models = load_models(&ModelPaths::from_dir(&models_dir))?;
    let image = image::open(&image_path).map_err(zeidex_ocr::OcrError::Image)?;
    let lines = detect_text_lines(&models, &image)?;

    let mut actual = Vec::new();
    for line in &lines {
        let (text, confidence) = recognize_line(&models, &image, *line)?;
        eprintln!("rec {:.3} {:?}", confidence, text);
        actual.push(normalize(&text));
    }

    let mut remaining = EXPECTED.to_vec();
    let mut hits = 0;
    for text in &actual {
        if let Some(index) = remaining.iter().position(|expected| expected == text) {
            remaining.remove(index);
            hits += 1;
        }
    }

    // Detection order follows the page, not the grid, so compare sorted
    // multisets: a reordered cell is not a recognition error.
    let mut expected_sorted = EXPECTED.to_vec();
    expected_sorted.sort_unstable();
    let mut actual_sorted = actual.clone();
    actual_sorted.sort_unstable();
    let expected_text = expected_sorted.join(" ");
    let actual_text = actual_sorted.join(" ");
    let distance = levenshtein(&expected_text, &actual_text);
    let cer = distance as f64 / expected_text.chars().count() as f64;
    eprintln!(
        "exact cells: {hits}/{}; CER {cer:.3}; unmatched: {remaining:?}",
        EXPECTED.len()
    );

    assert_eq!(
        hits,
        EXPECTED.len(),
        "recognition missed cells: {remaining:?} (recognized: {actual:?})"
    );
    assert!(cer <= 0.02, "CER {cer:.3} exceeds 2%");
    Ok(())
}
