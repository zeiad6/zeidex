//! The writer and the reader must agree on what a document is.
//!
//! `zeidex-exporter` turns a recognized page into `.docx`/`.xlsx`; the Studio
//! now opens those same formats. A hand-written fixture in `zeidex-ocr` proves
//! the reader handles the XML I thought of. This proves it handles the XML we
//! actually ship, including the table and the Arabic title the exporter writes.

use zeidex_exporter::{docx, xlsx};
use zeidex_ocr::{
    BBox, PageLayout, PageResult, Region, RegionKind, TableCell, TableStructure, extract_document,
};

fn table() -> TableStructure {
    TableStructure {
        bbox: BBox::new(0.0, 20.0, 100.0, 60.0),
        rows: 2,
        cols: 2,
        cells: vec![
            TableCell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(0.0, 0.0, 50.0, 10.0),
                text: "البند".to_string(),
            },
            TableCell {
                row: 0,
                col: 1,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(50.0, 0.0, 100.0, 10.0),
                text: "Amount".to_string(),
            },
            TableCell {
                row: 1,
                col: 0,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(0.0, 10.0, 50.0, 20.0),
                text: "خدمة".to_string(),
            },
            TableCell {
                row: 1,
                col: 1,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(50.0, 10.0, 100.0, 20.0),
                text: "1200".to_string(),
            },
        ],
    }
}

fn page(index: usize, title: &str) -> PageResult {
    PageResult {
        page_index: index,
        width: 200,
        height: 100,
        layout: PageLayout {
            width: 200,
            height: 100,
            regions: vec![
                Region {
                    kind: RegionKind::Title,
                    bbox: BBox::new(0.0, 0.0, 100.0, 20.0),
                    confidence: 0.9,
                    text: Some(title.to_string()),
                    table: None,
                },
                Region {
                    kind: RegionKind::Table,
                    bbox: BBox::new(0.0, 20.0, 100.0, 60.0),
                    confidence: 0.8,
                    text: None,
                    table: Some(table()),
                },
            ],
        },
        text: title.to_string(),
    }
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

#[test]
fn a_docx_we_write_is_a_docx_we_can_read() {
    let dir = temp_dir();
    let path = dir.path().join("تقرير.docx");
    docx::export_docx_pages(
        &[page(0, "تقرير المبيعات"), page(1, "Quarterly revenue")],
        &path,
    )
    .expect("export docx");

    let pages = extract_document(&path).expect("read back");
    assert_eq!(
        pages.len(),
        1,
        "Word did not record a page break; one page is honest"
    );
    let text = &pages[0].text;
    assert!(text.contains("تقرير المبيعات"), "{text}");
    assert!(text.contains("Quarterly revenue"), "{text}");
    assert!(text.contains("البند"), "table cell missing: {text}");
    assert!(text.contains("1200"), "table cell missing: {text}");
}

#[test]
fn an_xlsx_we_write_is_an_xlsx_we_can_read() {
    let dir = temp_dir();
    let path = dir.path().join("report.xlsx");
    xlsx::export_xlsx_pages(&[page(0, "تقرير المبيعات")], &path).expect("export xlsx");

    let pages = extract_document(&path).expect("read back");
    assert_eq!(pages.len(), 1, "{pages:?}");
    let text = &pages[0].text;
    assert!(text.contains("تقرير المبيعات"), "{text}");
    assert!(text.contains("Amount"), "table cell missing: {text}");
    assert!(text.contains("1200"), "table cell missing: {text}");
}
