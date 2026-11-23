//! A/B measurement of an alternate Arabic recognizer against the bundled one.
//!
//! The corpus in `tests/fixtures/arabic_corpus/` is **rendered text**, not a
//! photographed scan: it is clean, high-contrast and free of perspective. It is
//! a real labelled set and a real error rate, but it cannot answer the question
//! "does this recognizer read a phone photo of a Syrian ID card better?" — that
//! needs scans with ground truth. This harness exists so that when such a corpus
//! arrives, the comparison is one command and not a rewrite.
//!
//! Runs only when both directories are given:
//!
//! ```text
//! ZEIDEX_TEST_MODELS_DIR=/path/to/bundled/models \
//! ZEIDEX_TEST_AR_MODELS_DIR=/path/to/candidate/models \
//! cargo test --manifest-path core/Cargo.toml -p zeidex-ocr \
//!   --test arabic_recognizer_ab -- --nocapture
//! ```
//!
//! `ZEIDEX_TEST_AR_VISUAL=1` marks the candidate's Arabic recognizer as
//! visual-order (PP-OCRv5 exports are; the bundled v4 one is logical-order).
//!
//! The test asserts the contract — the candidate must read every corpus line —
//! and prints both rates and their difference. It deliberately does **not**
//! assert which is better: adoption is a decision recorded in
//! `ENGINEERING_REPORT_0.4.0.md`, taken on evidence, not on a test that would
//! fail the moment a model improves.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use zeidex_ocr::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, ModelSet, detect_text_lines, load_models, recognize_line};

#[derive(Deserialize)]
struct GroundTruth {
    size: u32,
    lines: Vec<TruthLine>,
}

#[derive(Deserialize)]
struct TruthLine {
    file: String,
    text: String,
}

/// Error counts for one pass over the corpus.
#[derive(Default)]
struct Rates {
    characters: usize,
    character_errors: f64,
    words: usize,
    word_errors: f64,
    lines_read: usize,
}

impl Rates {
    fn cer(&self) -> f64 {
        if self.characters == 0 {
            0.0
        } else {
            self.character_errors / self.characters as f64
        }
    }

    fn wer(&self) -> f64 {
        if self.words == 0 {
            0.0
        } else {
            self.word_errors / self.words as f64
        }
    }
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/arabic_corpus")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

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
                1 + previous.min(row[j]).min(row[j + 1])
            };
            previous = current;
        }
    }
    row[b.len()]
}

fn error_rate<T: PartialEq>(expected: &[T], produced: &[T]) -> f64 {
    if expected.is_empty() {
        return if produced.is_empty() { 0.0 } else { 1.0 };
    }
    levenshtein(expected, produced) as f64 / expected.len() as f64
}

fn measure(models: &ModelSet, dir: &Path, truth: &GroundTruth) -> OcrResult<Rates> {
    let mut rates = Rates::default();
    for line in &truth.lines {
        let image = image::open(dir.join(&line.file)).map_err(zeidex_ocr::OcrError::Image)?;
        let boxes = detect_text_lines(models, &image)?;
        let mut produced = String::new();
        for box_ in &boxes {
            let (text, _confidence) = recognize_line(models, &image, *box_)?;
            if !produced.is_empty() {
                produced.push(' ');
            }
            produced.push_str(&text);
        }
        let produced = collapse(&produced);
        let expected = collapse(&line.text);
        if !produced.is_empty() {
            rates.lines_read += 1;
        }
        rates.characters += expected.chars().count();
        rates.character_errors +=
            error_rate(&chars(&expected), &chars(&produced)) * expected.chars().count() as f64;
        rates.words += words(&expected).len();
        rates.word_errors +=
            error_rate(&words(&expected), &words(&produced)) * words(&expected).len() as f64;
    }
    Ok(rates)
}

fn report(label: &str, rates: &Rates, lines: usize) {
    eprintln!(
        "{label:<24} cer={:6.2}% wer={:6.2}% lines_read={}/{}",
        rates.cer() * 100.0,
        rates.wer() * 100.0,
        rates.lines_read,
        lines
    );
}

#[test]
fn an_alternate_arabic_recognizer_is_measured_against_the_bundled_one() -> OcrResult<()> {
    let Ok(baseline_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(candidate_dir) = std::env::var("ZEIDEX_TEST_AR_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_AR_MODELS_DIR not set");
        return Ok(());
    };
    let dir = corpus_dir();
    let truth: GroundTruth =
        serde_json::from_str(&std::fs::read_to_string(dir.join("ground_truth.json"))?)
            .expect("ground_truth.json is well formed");
    assert!(!truth.lines.is_empty(), "the corpus is empty");

    let baseline = load_models(&ModelPaths::from_dir(&baseline_dir))?;
    let mut candidate_paths = ModelPaths::from_dir(&candidate_dir);
    if std::env::var("ZEIDEX_TEST_AR_VISUAL").is_ok_and(|value| value == "1") {
        candidate_paths = candidate_paths.with_visual_arabic();
    }
    let candidate = load_models(&candidate_paths)?;

    eprintln!(
        "corpus: {} lines, {}px, rendered (not photographed)",
        truth.lines.len(),
        truth.size
    );
    let base = measure(&baseline, &dir, &truth)?;
    let cand = measure(&candidate, &dir, &truth)?;
    report("bundled (v4)", &base, truth.lines.len());
    report("candidate", &cand, truth.lines.len());
    eprintln!(
        "delta (candidate - bundled): cer={:+.2} pp, wer={:+.2} pp",
        (cand.cer() - base.cer()) * 100.0,
        (cand.wer() - base.wer()) * 100.0
    );

    // Contract: a recognizer that returns nothing is not a result to compare.
    assert_eq!(
        cand.lines_read,
        truth.lines.len(),
        "the candidate read {} of {} corpus lines",
        cand.lines_read,
        truth.lines.len()
    );
    Ok(())
}
