//! The report is a promise to a spreadsheet user: four sheets, a real table,
//! numbers stored as numbers. These tests open the produced package and check
//! the XML that carries those promises.

// Tests are allowed to fail loudly: the crate deny-list applies to shipped code.
#![allow(clippy::expect_used)]

use std::io::Read;
use std::path::Path;

use zeidex_batch::matcher::{BatchFile, FileKind, search};
use zeidex_batch::report::{ReportMeta, export_report};

fn zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open package");
    let mut zip = zip::ZipArchive::new(file).expect("open zip");
    let mut entry = zip.by_name(name).expect("entry");
    let mut contents = String::new();
    entry.read_to_string(&mut contents).expect("read entry");
    contents
}

fn zip_names(path: &Path) -> Vec<String> {
    let file = std::fs::File::open(path).expect("open package");
    let zip = zip::ZipArchive::new(file).expect("open zip");
    zip.file_names()
        .map(|name| name.expect("entry name").into_owned())
        .collect()
}

/// One batch with every outcome the report has a sheet for.
fn report() -> zeidex_batch::BatchReport {
    let files = vec![
        BatchFile::new(
            "/home/user/Documents/invoice-2024-final.pdf",
            "invoice-2024-final.pdf",
            FileKind::File,
            Some(1_700_000_000),
        ),
        BatchFile::new(
            "/home/user/Downloads/quarterly-report.pdf",
            "quarterly-report.pdf",
            FileKind::File,
            Some(1_700_000_000),
        ),
        BatchFile::new(
            "/home/user/Archive/report.pdf",
            "report.pdf",
            FileKind::File,
            Some(1_700_000_000),
        ),
        // The same name in two folders: one request, two candidates.
        BatchFile::new(
            "/home/user/Downloads/report.pdf",
            "report.pdf",
            FileKind::File,
            Some(1_700_000_000),
        ),
        BatchFile::new(
            "/home/user/Documents/مدرسة-الأطفال.pdf",
            "مدرسة-الأطفال.pdf",
            FileKind::File,
            Some(1_700_000_000),
        ),
    ];
    let entries = [
        "invoice".to_string(),
        "quarterly-reprot.pdf".to_string(),
        "report.pdf".to_string(),
        "مدرسه-الاطفال.pdf".to_string(),
        "missing-notes.docx".to_string(),
    ];
    search(&files, &entries, 70)
}

fn meta() -> ReportMeta<'static> {
    ReportMeta {
        generated_unix_s: 1_700_000_000,
        app_version: "0.1.0",
    }
}

#[test]
fn the_workbook_has_the_four_sheets_the_report_promises() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let workbook = zip_entry(&path, "xl/workbook.xml");
    for sheet in ["Summary", "Results", "Missing", "Multiple Matches"] {
        assert!(
            workbook.contains(&format!("name=\"{sheet}\"")),
            "missing sheet {sheet}: {workbook}"
        );
    }
    // Every sheet has to be a real part, not just a name in the workbook.
    let names = zip_names(&path);
    for index in 1..=4 {
        assert!(
            names.contains(&format!("xl/worksheets/sheet{index}.xml")),
            "missing sheet{index}.xml: {names:?}"
        );
    }
}

#[test]
fn the_summary_reports_the_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let strings = zip_entry(&path, "xl/sharedStrings.xml");
    for expected in [
        "Batch File Search Report",
        "Requested names",
        "Success rate",
        "Matching threshold",
        "Search scope",
        "5 indexed files and folders",
    ] {
        assert!(
            strings.contains(expected),
            "summary is missing {expected}: {strings}"
        );
    }
    // The chart is an inserted drawing, not a promise of one.
    let sheet = zip_entry(&path, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<drawing") || sheet.contains("drawing"),
        "summary has no chart: {sheet}"
    );
}

#[test]
fn the_results_sheet_is_filterable_frozen_and_percentaged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let sheet = zip_entry(&path, "xl/worksheets/sheet2.xml");
    assert!(
        sheet.contains("autoFilter"),
        "results are not filterable: {sheet}"
    );
    assert!(sheet.contains("<pane"), "results are not frozen: {sheet}");

    let styles = zip_entry(&path, "xl/styles.xml");
    assert!(
        styles.contains("numFmt") && styles.contains("0%"),
        "percentages have no percent format: {styles}"
    );

    let strings = zip_entry(&path, "xl/sharedStrings.xml");
    for header in [
        "Requested Filename",
        "Matched Filename",
        "Match Type",
        "File Path",
    ] {
        assert!(
            strings.contains(header),
            "missing column {header}: {strings}"
        );
    }
    // A name that matched nothing still appears in the flat table.
    assert!(
        strings.contains("missing-notes.docx"),
        "a not-found request was silently dropped: {strings}"
    );
}

#[test]
fn arabic_names_survive_into_the_workbook() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let strings = zip_entry(&path, "xl/sharedStrings.xml");
    assert!(
        strings.contains("مدرسة-الأطفال.pdf"),
        "Arabic filename was mangled: {strings}"
    );
    assert!(
        strings.contains("مدرسه-الاطفال.pdf"),
        "Arabic request was mangled: {strings}"
    );
}

#[test]
fn the_missing_sheet_lists_only_what_was_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let sheet = zip_entry(&path, "xl/worksheets/sheet3.xml");
    // Header plus one row: the only request with no candidate.
    assert!(
        sheet.contains("<row r=\"2\""),
        "missing sheet has no data row: {sheet}"
    );
    assert!(
        !sheet.contains("<row r=\"3\""),
        "missing sheet lists a file that was found: {sheet}"
    );
}

#[test]
fn the_multiple_sheet_lists_every_candidate_of_a_repeated_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("batch.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let sheet = zip_entry(&path, "xl/worksheets/sheet4.xml");
    // `report.pdf` exists twice, so the sheet has a header and two rows.
    assert!(
        sheet.contains("<row r=\"2\"") && sheet.contains("<row r=\"3\""),
        "multiple-match sheet is incomplete: {sheet}"
    );
}

#[test]
fn an_empty_batch_still_produces_a_valid_workbook() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("empty.xlsx");
    let report = search(&[], &[], 70);
    export_report(&report, &meta(), &path).expect("export");

    let workbook = zip_entry(&path, "xl/workbook.xml");
    assert!(workbook.contains("name=\"Results\""));
    // The empty-state note is written, so the sheet is not blank.
    let strings = zip_entry(&path, "xl/sharedStrings.xml");
    assert!(
        strings.contains("Every requested name was found."),
        "missing empty state: {strings}"
    );
}

#[test]
fn a_report_survives_a_path_with_spaces_and_unicode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("تقرير الدفعة 2026.xlsx");
    export_report(&report(), &meta(), &path).expect("export");
    assert!(path.exists());
}

/// Excel refuses a cell over 32 767 UTF-16 units, and the library reports that
/// as an error that aborts the export. A pasted paragraph with no line breaks is
/// a single entry, so one long request used to cost the whole workbook: no file,
/// no warning, and a cryptic `MaxStringLengthExceeded`.
#[test]
fn one_enormous_request_does_not_cost_the_whole_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("long.xlsx");
    let huge = "ط".repeat(40_000);
    let entries = [huge, "missing-notes.docx".to_string()];
    let report = search(&[], &entries, 70);
    export_report(&report, &meta(), &path).expect("a long request must not abort the export");
    assert!(path.exists());

    // The workbook is complete, not merely present.
    let workbook = zip_entry(&path, "xl/workbook.xml");
    for sheet in ["Summary", "Results", "Missing", "Multiple Matches"] {
        assert!(workbook.contains(&format!("name=\"{sheet}\"")), "{sheet}");
    }
}

/// The chart's four bars are the mutually exclusive outcomes. `Found` is the sum
/// of the two match rows, so plotting it alongside them made the chart report
/// more outcomes than there were requests.
#[test]
fn the_chart_plots_only_mutually_exclusive_outcomes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("chart.xlsx");
    export_report(&report(), &meta(), &path).expect("export");

    let chart = zip_entry(&path, "xl/charts/chart1.xml");
    // Rows 6..9 (zero-based) are Exact, Approximate, Not found and Errors: the
    // four outcomes that partition the batch.
    assert!(
        chart.contains("<c:f>Summary!$B$7:$B$10</c:f>"),
        "chart values must cover the outcome rows: {chart}"
    );
    assert!(
        chart.contains("<c:f>Summary!$A$7:$A$10</c:f>"),
        "chart categories must cover the outcome rows: {chart}"
    );
}

/// Not a timing assertion: this only proves a batch far larger than a person
/// pastes still finishes. Run with `--ignored --nocapture` to see the number.
#[test]
#[ignore = "scale check, run explicitly"]
fn a_large_batch_finishes() {
    let files: Vec<BatchFile> = (0..20_000)
        .map(|index| {
            let name = format!("document-{index:05}-report-final.pdf");
            BatchFile::new(
                format!("/archive/{name}"),
                name,
                FileKind::File,
                Some(1_700_000_000),
            )
        })
        .collect();
    let entries: Vec<String> = (0..1_000)
        .map(|index| format!("document-{:05}-report-fnal.pdf", index * 7))
        .collect();

    let started = std::time::Instant::now();
    let report = search(&files, &entries, 70);
    let elapsed = started.elapsed();
    eprintln!(
        "1000 names x 20000 files: {:?} ({} found, {} ms reported)",
        elapsed, report.summary.found, report.summary.duration_ms
    );
    assert_eq!(report.summary.total, 1_000);
    assert!(
        report.summary.found >= 900,
        "fuzzy matching lost too many known files: {} found",
        report.summary.found
    );
}
