//! Writes a sample report, for looking at the workbook with a spreadsheet
//! program instead of reading XML.
//!
//! ```text
//! cargo run -p zeidex-batch --example write_report -- /tmp/zeidex-batch.xlsx
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use zeidex_batch::matcher::{BatchFile, FileKind, search};
use zeidex_batch::report::{ReportMeta, export_report};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "zeidex-batch.xlsx".to_string());

    let folders = [
        "Documents",
        "Downloads",
        "Archive/2024",
        "Documents/Invoices",
    ];
    let names = [
        "invoice-2024-final.pdf",
        "invoice-2024-014.pdf",
        "invoice-2023-summary.xlsx",
        "quarterly-report.pdf",
        "quarterly-report-Q3.docx",
        "report.pdf",
        "meeting-notes-2026-03.docx",
        "budget-2026.xlsx",
        "project-plan.pdf",
        "contract-signed.pdf",
        "notes.txt",
        "README.md",
        "تقرير-الاجتماع.docx",
        "فاتورة-2024.pdf",
        "مدرسة-الأطفال.pdf",
        "كتاب-الطالب.pdf",
        "خطة-المشروع-2026.pdf",
    ];

    // The report stats each returned match to show its size, so the sample
    // needs real files. They are written into a temporary tree with a size that
    // varies by name, which is what makes the Size column worth reading.
    let root = std::env::temp_dir().join("zeidex-batch-sample");
    let _ = std::fs::remove_dir_all(&root);

    // Enough rows that a filter is worth having, with the same names repeating
    // in different folders, which is what makes "multiple matches" real.
    let mut files = Vec::new();
    for (round, folder) in (0..6).flat_map(|round| folders.iter().map(move |f| (round, f))) {
        let directory = root.join(folder).join(format!("batch-{round}"));
        std::fs::create_dir_all(&directory).expect("create sample folder");
        for (index, name) in names.iter().enumerate() {
            let path = directory.join(name);
            let size = 512 * (index + 1) + round * 97;
            std::fs::write(&path, vec![b'x'; size]).expect("write sample file");
            files.push(BatchFile::new(
                path.to_string_lossy(),
                *name,
                FileKind::File,
                Some(1_700_000_000 + round as i64 * 86_400),
            ));
        }
    }
    let invoices = root.join("Documents/Invoices");
    std::fs::create_dir_all(&invoices).expect("create sample folder");
    files.push(BatchFile::new(
        invoices.to_string_lossy(),
        "Invoices",
        FileKind::Folder,
        Some(1_700_000_000),
    ));

    let entries: Vec<String> = vec![
        "invoice-2024-final.pdf",
        "invoice-2024-014.pdf",
        "quarterly-report.pdf",
        "report.pdf",
        "meeting-notes.docx",
        "budget-2026.xlsx",
        "contract-2025.pdf",
        "notes.txt",
        "تقرير-الاجتماع.docx",
        "فاتورة-2024.pdf",
        "مدرسه-الاطفال.pdf",
        "خطة-المشروع.pdf",
        "missing-file.txt",
        "readme.md",
        "Invoices",
        "project-plan-2026.pdf",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    let report = search(&files, &entries, 70);
    let generated = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    let meta = ReportMeta {
        generated_unix_s: generated,
        app_version: env!("CARGO_PKG_VERSION"),
    };
    export_report(&report, &meta, &path).expect("write report");
    println!(
        "wrote {path}: {} entries, {} found, {} ms",
        report.summary.total, report.summary.found, report.summary.duration_ms
    );
}
