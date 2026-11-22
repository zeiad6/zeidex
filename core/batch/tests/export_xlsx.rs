//! End-to-end check of the Excel report: a real search result, written to a
//! real file.
//!
//! The unit tests in `report.rs` cover the cell text; this one covers the file
//! itself, because a workbook that fails to save is not a report. The sheet
//! contents are verified from outside by `scripts/verify-xlsx.py`, which opens
//! the file with an independent reader.

use std::path::PathBuf;

use zeidex_batch::matcher::{BatchFile, FileKind, search};
use zeidex_batch::report::{ReportMeta, export_report};

fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

fn sample_files() -> Vec<BatchFile> {
    // `BatchFile::new` takes the name the index stored; the path is only used
    // for display, for opening the result, and for the size lookup the matcher
    // does on the matches it returns.
    let file =
        |path: &str, name: &str| BatchFile::new(path, name, FileKind::File, Some(1_700_000_000));
    vec![
        file("/home/user/Documents/report.pdf", "report.pdf"),
        file(
            "/home/user/Downloads/invoice_2024.xlsx",
            "invoice_2024.xlsx",
        ),
        // The same name in two folders: a "multiple matches" case.
        file("/home/user/Archive/report.pdf", "report.pdf"),
        file(
            "/home/user/Documents/تقرير الاجتماع.docx",
            "تقرير الاجتماع.docx",
        ),
        file(
            "/home/user/Downloads/project_final.docx",
            "project_final.docx",
        ),
    ]
}

#[test]
fn a_search_result_is_written_as_a_readable_workbook() {
    let entries = vec![
        "report.pdf".to_string(),
        "invoice 2024".to_string(),
        "project-final.docx".to_string(),
        "تقرير الاجتماع.docx".to_string(),
        "not-on-this-disk-at-all.txt".to_string(),
        // Only tatweel: nothing searchable survives normalization, so this is
        // an error row rather than a missing file.
        "\u{0640}\u{0640}".to_string(),
    ];
    let report = search(&sample_files(), &entries, 70);
    assert_eq!(report.entries.len(), entries.len());
    assert!(
        report.summary.multiple >= 1,
        "the fixture must contain a name with several candidates"
    );
    assert_eq!(report.summary.errors, 1);

    let path = target_dir().join("zeidex-batch-report.xlsx");
    let _ = std::fs::remove_file(&path);
    export_report(
        &report,
        &ReportMeta {
            generated_unix_s: 1_700_000_000,
            app_version: "test",
        },
        &path,
    )
    .expect("the report should be written");

    let bytes = std::fs::read(&path).expect("the report should exist");
    assert!(
        bytes.len() > 8_000,
        "a styled workbook is not a stub: {bytes:?}"
    );
    assert_eq!(&bytes[..2], b"PK", "an xlsx is a zip container");

    // Overwriting an existing report must work: the user picks the same name
    // twice more often than not.
    export_report(
        &report,
        &ReportMeta {
            generated_unix_s: 1_700_000_001,
            app_version: "test",
        },
        &path,
    )
    .expect("a second export over the same path should succeed");
    let again = std::fs::read(&path).expect("the overwritten report should exist");
    assert_eq!(&again[..2], b"PK");
}
