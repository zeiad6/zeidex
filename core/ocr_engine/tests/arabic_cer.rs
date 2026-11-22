//! Arabic OCR benchmark: character and word error rate on a fixed corpus.
//!
//! The corpus in `fixtures/arabic_corpus/` is rendered by
//! `tools/make-arabic-corpus.py` with a known font, size and pitch, and ships
//! with `ground_truth.json`. Because the truth is known, the result is a real
//! CER/WER instead of "the pipeline returned something". That is the number to
//! quote when a preprocessing, dictionary or model change is proposed.
//!
//! Two rates are reported per line:
//!
//! - **raw** — what the recognizer produced, after collapsing whitespace. This
//!   is the honest OCR quality number.
//! - **search** — the same comparison after `normalize_for_search`, the folding
//!   layer search applies to *both* sides at query time. It is not applied to
//!   the stored text; it only shows how much of the remaining error the search
//!   layer already absorbs (ta marbuta, alef forms, Arabic-Indic digits).
//!
//! Skips unless `ZEIDEX_TEST_MODELS_DIR` names a model directory and
//! `ORT_DYLIB_PATH` resolves, exactly like the other real-model tests.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use zeidex_matching::normalize_for_search;
use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_text_lines, load_models, recognize_line};

#[derive(Deserialize)]
struct GroundTruth {
    size: u32,
    lines: Vec<TruthLine>,
}

#[derive(Deserialize)]
struct TruthLine {
    file: String,
    text: String,
    #[serde(default)]
    font: String,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/arabic_corpus")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Edit distance between two sequences of units (characters for CER, words for
/// WER). One substitution, insertion or deletion costs one unit, which is the
/// definition both rates are quoted against.
fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, unit) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, other) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = if unit == other {
                previous
            } else {
                1 + previous.min(row[j]).min(current)
            };
            previous = current;
        }
    }
    row[b.len()]
}

/// Error rate as a fraction, guarding the empty-truth case.
fn error_rate(truth: &str, produced: &str, unit: impl Fn(&str) -> Vec<String>) -> f64 {
    let truth_units = unit(truth);
    if truth_units.is_empty() {
        return if produced.is_empty() { 0.0 } else { 1.0 };
    }
    let produced_units = unit(produced);
    levenshtein(&truth_units, &produced_units) as f64 / truth_units.len() as f64
}

fn chars(text: &str) -> Vec<String> {
    text.chars().map(|c| c.to_string()).collect()
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn arabic_corpus_reports_character_and_word_error_rates() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let dir = corpus_dir();
    let truth_path = dir.join("ground_truth.json");
    if !truth_path.exists() {
        eprintln!("skipping: {} is missing", truth_path.display());
        return Ok(());
    }
    let truth: GroundTruth = serde_json::from_str(&std::fs::read_to_string(&truth_path)?)
        .expect("ground_truth.json is well formed");

    let models = load_models(&ModelPaths::from_dir(&models_dir))?;

    let mut raw_chars = 0usize;
    let mut raw_char_errors = 0.0f64;
    let mut raw_word_errors = 0.0f64;
    let mut raw_words = 0usize;
    let mut search_char_errors = 0.0f64;
    let mut recognized_lines = 0usize;

    eprintln!("corpus: {} lines, {}px", truth.lines.len(), truth.size);

    // Provenance: every line records the font it was rendered with. A corpus
    // that silently loses that cannot be regenerated, and a line drawn in a
    // font without the needed glyphs measures tofu instead of recognition.
    for line in &truth.lines {
        assert!(
            !line.font.is_empty(),
            "{} declares no font, so it cannot be regenerated",
            line.file
        );
    }

    for line in &truth.lines {
        let path = dir.join(&line.file);
        let image = image::open(&path).map_err(zeidex_ocr::OcrError::Image)?;
        let boxes = detect_text_lines(&models, &image)?;

        // One line per image is the corpus contract. A detector that splits or
        // merges it is itself a result worth reporting, so join what it found
        // and let the error rate show the damage.
        let mut produced = String::new();
        let mut worst_confidence: f32 = 1.0;
        for box_ in &boxes {
            let (text, confidence) = recognize_line(&models, &image, *box_)?;
            if !produced.is_empty() {
                produced.push(' ');
            }
            produced.push_str(&text);
            worst_confidence = worst_confidence.min(confidence);
        }
        let produced = collapse(&produced);
        let expected = collapse(&line.text);

        let char_rate = error_rate(&expected, &produced, chars);
        let word_rate = error_rate(&expected, &produced, words);
        let search_rate = error_rate(
            &normalize_for_search(&expected),
            &normalize_for_search(&produced),
            chars,
        );

        raw_chars += expected.chars().count();
        raw_words += words(&expected).len();
        raw_char_errors += char_rate * expected.chars().count() as f64;
        raw_word_errors += word_rate * words(&expected).len() as f64;
        search_char_errors += search_rate * expected.chars().count() as f64;
        if !produced.is_empty() {
            recognized_lines += 1;
        }

        eprintln!(
            "{:<18} cer={:6.2}% wer={:6.2}% search-cer={:6.2}% conf={:.3} boxes={} {:?}",
            line.file,
            char_rate * 100.0,
            word_rate * 100.0,
            search_rate * 100.0,
            worst_confidence,
            boxes.len(),
            produced
        );
    }

    let aggregate_cer = raw_char_errors / raw_chars as f64;
    let aggregate_wer = raw_word_errors / raw_words as f64;
    let aggregate_search_cer = search_char_errors / raw_chars as f64;
    eprintln!(
        "aggregate: cer={:.2}% wer={:.2}% search-cer={:.2}% over {} characters, {} words",
        aggregate_cer * 100.0,
        aggregate_wer * 100.0,
        aggregate_search_cer * 100.0,
        raw_chars,
        raw_words
    );

    // Contract, not a quality claim: every corpus image must produce text, and
    // the measured rates are pinned with headroom so a regression is loud while
    // a model upgrade that improves things does not fail the build.
    assert_eq!(
        recognized_lines,
        truth.lines.len(),
        "every corpus image must produce recognized text"
    );
    assert!(
        aggregate_cer <= MAX_CER,
        "Arabic CER regressed: {:.2}% > {:.2}%",
        aggregate_cer * 100.0,
        MAX_CER * 100.0
    );
    assert!(
        aggregate_wer <= MAX_WER,
        "Arabic WER regressed: {:.2}% > {:.2}%",
        aggregate_wer * 100.0,
        MAX_WER * 100.0
    );
    Ok(())
}

/// Ceilings from the measured baseline on PP-OCRv4 Arabic rec, rounded up so
/// ordinary model-file changes do not trip them. Lower them when the baseline
/// improves; never raise one without saying which regression it hides.
///
/// 0.2.0 measured 15.57 % / 31.03 % with whole-line recognition. Word-level
/// re-reading of low-confidence lines (0.3.0) took that to 0.60 % / 3.45 %, and
/// these ceilings were lowered to match instead of being left at the old
/// values, which would have hidden a return to the old behaviour.
const MAX_CER: f64 = 0.08;
const MAX_WER: f64 = 0.15;
