//! A measured batch search over a realistic index size.
//!
//! This is a performance test, not a correctness one: it prints what a search
//! costs so a regression is visible, and asserts only a generous ceiling so a
//! slow CI machine does not fail the build. Run it with `--release --nocapture`
//! to get the numbers quoted in the engineering report.

use std::time::Instant;

use zeidex_batch::matcher::{BatchFile, FileKind, search};

/// 100,000 files spread over 100 folders, with names that repeat a stem and
/// differ by a serial, which is what a real document archive looks like.
fn corpus(count: usize) -> Vec<BatchFile> {
    const STEMS: [&str; 8] = [
        "report", "invoice", "contract", "scan", "photo", "meeting", "budget", "letter",
    ];
    (0..count)
        .map(|i| {
            let name = format!("{}_{i:05}.pdf", STEMS[i % STEMS.len()]);
            let path = format!("/archive/{:02}/{name}", i % 100);
            BatchFile::new(path, name, FileKind::File, Some(1_700_000_000))
        })
        .collect()
}

/// Peak resident set of this process, in MiB.
///
/// Reported because a batch search holds the whole index-derived corpus in
/// memory at once, and "fast" is not an achievement if it costs a gigabyte.
/// Linux keeps the high-water mark; elsewhere the line is simply omitted.
fn peak_rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

fn run(label: &str, files: &[BatchFile], entries: &[String], threshold: u8) {
    let started = Instant::now();
    let report = search(files, entries, threshold);
    let elapsed = started.elapsed();
    eprintln!(
        "{label}: {} names over {} files at {threshold}% -> {} ms total, {} ms/name, found {}, missing {}",
        entries.len(),
        files.len(),
        elapsed.as_millis(),
        elapsed.as_millis() as f64 / entries.len().max(1) as f64,
        report.summary.found,
        report.summary.not_found,
    );
    assert!(
        elapsed.as_secs() < 30,
        "{label} took {elapsed:?}, which is far past interactive"
    );
}

#[test]
#[ignore = "scale check: run with --release -- --ignored --nocapture"]
fn a_large_batch_stays_interactive() {
    let files = corpus(100_000);

    // Names that are in the index: the exact path, a hash lookup per entry.
    // Every eighth file is a `report_*`, so these names all exist.
    let hits: Vec<String> = (0..200)
        .map(|i| format!("report_{:05}.pdf", i * 8))
        .collect();
    run("exact", &files, &hits, 100);

    // Names that are not in the index: every one scans the corpus, which is the
    // expensive case a batch of typos hits.
    let misses: Vec<String> = (0..200)
        .map(|i| format!("quarterly-summary-{i}-final.docx"))
        .collect();
    run("misses", &files, &misses, 70);

    // Near misses: a typo in a name that exists, the fuzzy path users expect.
    let typos: Vec<String> = (0..200)
        .map(|i| format!("reprot_{:05}.pdf", i * 8))
        .collect();
    run("typos", &files, &typos, 70);

    // A broad threshold is the widest net: more candidates survive, so this is
    // the slowest honest configuration.
    let broad: Vec<String> = (0..200).map(|i| format!("meeting notes {i}")).collect();
    run("broad", &files, &broad, 50);

    match peak_rss_mb() {
        Some(mb) => eprintln!(
            "peak resident set with a {} file corpus: {mb} MiB",
            files.len()
        ),
        None => eprintln!("peak resident set: not measurable on this platform"),
    }
}
