//! Search-latency report for the name-matching tiers (exact/prefix/suffix,
//! subsequence, and the typo-tolerant pass).
//!
//! Not part of the normal suite: it builds tens of thousands of candidates and
//! times the whole search path, so it is meant to be run explicitly in release
//! mode.
//!
//! ```text
//! cargo test -p zeidex-engine --release --test match_latency -- --ignored --nocapture
//! ```
//!
//! The typo query is the interesting number: it is the only case where the
//! approximate matcher runs for candidates nothing else matched, so its cost is
//! the price of the whole feature.

use std::time::Instant;

use zeidex_engine::QueryEngine;
use zeidex_indexing::{Candidate, CandidateKind};

const ITERATIONS: usize = 60;

fn build_engine(count: usize) -> QueryEngine {
    let mut candidates = Vec::with_capacity(count);
    for index in 0..count {
        let title = match index % 8 {
            0 => format!("invoice-2026-{index:05}.pdf"),
            1 => format!("annual-report-{index:04}.xlsx"),
            2 => format!("تقرير-الاجتماع-{index:04}.pdf"),
            3 => format!("IMG_{index:04}.jpg"),
            4 => format!("meeting-notes-{index:04}.md"),
            5 => format!("project-plan-{index:04}.docx"),
            6 => format!("budget-{index:04}.csv"),
            _ => format!("photo-{index:04}.png"),
        };
        let path = format!("/home/user/Documents/{title}");
        candidates.push(Candidate::new(
            &format!("file:{index}"),
            CandidateKind::File,
            &title,
            &path,
        ));
    }
    QueryEngine::new(candidates)
}

fn measure(engine: &QueryEngine, query: &str) -> (u128, u128, usize) {
    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut hits = 0usize;
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let results = engine.search(query, 10);
        samples.push(started.elapsed().as_micros());
        hits = hits.max(results.len());
    }
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    (p50, p95, hits)
}

#[test]
#[ignore = "latency benchmark: run in release with -- --ignored --nocapture"]
fn search_latency_report() {
    println!("candidates,query,kind,p50_us,p95_us,results");

    for count in [8_000usize, 50_000] {
        let engine = build_engine(count);
        // "invoice" starts a name (prefix), "2026-01234" ends one (suffix),
        // "invoise" is one edit away from many names (approximate), and
        // "zzzzzzz" matches nothing at all.
        let cases = [
            ("invoice", "prefix"),
            ("2026-01234", "suffix"),
            ("report", "substring"),
            ("invoise", "approximate"),
            ("zzzzzzz", "no-match"),
        ];
        for (query, kind) in cases {
            let (p50, p95, results) = measure(&engine, query);
            println!("{count},{query},{kind},{p50},{p95},{results}");
        }
    }

    // A direct measurement of the matcher alone, without the rest of the search
    // path, so the added cost is attributable.
    let mut matcher = zeidex_matching::ApproximateMatcher::new("invoise").expect("eligible query");
    // These titles pass the first-letter gate and the length gate but match no
    // word, so this is the matcher's worst case: every word is measured.
    let titles: Vec<String> = (0..50_000)
        .map(|index| format!("impact-report-{index:04}.xlsx"))
        .collect();
    let started = Instant::now();
    let mut hits = 0usize;
    for title in &titles {
        if matcher.score(title).is_some() {
            hits += 1;
        }
    }
    let elapsed = started.elapsed();
    println!(
        "approximate_only,50000,{:.3}ms,hits={hits}",
        elapsed.as_secs_f64() * 1000.0
    );
}
