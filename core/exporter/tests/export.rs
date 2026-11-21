//! Round-trip tests: build a page layout, export it, then open the resulting
//! Office package and assert the XML carries the structure we promised.

use std::io::Read;
use std::path::Path;

use zeidex_exporter::{docx, xlsx};
use zeidex_ocr::geometry::BBox;
use zeidex_ocr::layout::{PageLayout, Region, RegionKind};
use zeidex_ocr::pipeline::PageResult;
use zeidex_ocr::table::TableStructure;
use zeidex_ocr::table::reconstruct;

fn table() -> TableStructure {
    let row = BBox::new(0.0, 0.0, 100.0, 20.0);
    let col = |x0: f32, x1: f32| BBox::new(x0, 0.0, x1, 20.0);
    let mut structure = reconstruct(
        BBox::new(0.0, 0.0, 100.0, 20.0),
        &[row, row],
        &[col(0.0, 50.0), col(50.0, 100.0)],
        &[(0, 0, 1, 2)],
    );
    structure.cells[0].text = "الاسم".to_string();
    structure.cells[1].text = "1,234.50".to_string();
    structure
}

fn page() -> PageResult {
    let mut table_region = Region {
        kind: RegionKind::Table,
        bbox: BBox::new(0.0, 0.0, 100.0, 20.0),
        confidence: 0.99,
        text: None,
        table: None,
    };
    table_region.table = Some(table());

    PageResult {
        page_index: 0,
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
                    text: Some("تقرير المبيعات".to_string()),
                    table: None,
                },
                table_region,
            ],
        },
        text: String::new(),
    }
}

fn zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open package");
    let mut zip = zip::ZipArchive::new(file).expect("open zip");
    let mut entry = zip.by_name(name).expect("entry");
    let mut contents = String::new();
    entry.read_to_string(&mut contents).expect("read entry");
    contents
}

#[test]
fn docx_sets_bidi_and_writes_a_real_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("page.docx");
    docx::export_docx(&page(), &path).expect("export docx");

    let document = zip_entry(&path, "word/document.xml");
    assert!(
        document.contains("<w:bidi"),
        "missing RTL paragraph property: {document}"
    );
    assert!(
        document.contains("<w:tbl>"),
        "missing native table: {document}"
    );
    assert!(
        document.contains("w:gridSpan w:val=\"2\""),
        "missing merged cell span: {document}"
    );
    assert!(
        document.contains("الاسم"),
        "missing Arabic cell text: {document}"
    );
}

#[test]
fn xlsx_is_rtl_and_keeps_numbers_as_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("page.xlsx");
    xlsx::export_xlsx(&page(), &path).expect("export xlsx");

    let sheet = zip_entry(&path, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("rightToLeft=\"1\""),
        "sheet is not RTL: {sheet}"
    );
    assert!(
        sheet.contains("1234.5") || sheet.contains("1234.50"),
        "numeric cell lost its value: {sheet}"
    );
    assert!(
        sheet.contains("<v>1234.5</v>") || sheet.contains("<v>1234.50</v>"),
        "numeric cell was written as text: {sheet}"
    );
    assert!(sheet.contains("mergeCell"), "missing merged range: {sheet}");
}

/// Index of `needle` in the workbook's shared-string table.
fn shared_index(path: &Path, needle: &str) -> Option<usize> {
    let xml = zip_entry(path, "xl/sharedStrings.xml");
    for (index, chunk) in xml.split("<si>").skip(1).enumerate() {
        let after_tag = chunk.split("<t").nth(1)?;
        let text = after_tag.split('>').nth(1)?.split('<').next()?;
        if text == needle {
            return Some(index);
        }
    }
    None
}

/// Cell reference (`A1`) of the worksheet cell holding `needle`.
fn cell_ref_of(path: &Path, sheet: &str, needle: &str) -> Option<String> {
    let index = shared_index(path, needle)?;
    let xml = zip_entry(path, sheet);
    let marker = format!("<v>{index}</v>");
    for chunk in xml.split("<c ").skip(1) {
        if chunk.contains("t=\"s\"") && chunk.contains(&marker) {
            return chunk
                .split("r=\"")
                .nth(1)?
                .split('"')
                .next()
                .map(str::to_string);
        }
    }
    None
}

fn page_with(index: usize, text: &str) -> PageResult {
    PageResult {
        page_index: index,
        width: 200,
        height: 100,
        layout: PageLayout {
            width: 200,
            height: 100,
            regions: vec![Region {
                kind: RegionKind::Text,
                bbox: BBox::new(10.0, 10.0, 190.0, 40.0),
                confidence: 0.9,
                text: Some(text.to_string()),
                table: None,
            }],
        },
        text: text.to_string(),
    }
}

#[test]
fn docx_export_pages_keeps_every_page_and_breaks_between_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("pages.docx");
    let pages = [page_with(0, "الصفحة الأولى"), page_with(1, "الصفحة الثانية")];
    docx::export_docx_pages(&pages, &path).expect("export docx pages");

    let document = zip_entry(&path, "word/document.xml");
    assert!(document.contains("الصفحة الأولى"), "page 1 text lost");
    assert!(document.contains("الصفحة الثانية"), "page 2 text lost");
    assert!(
        document.contains("<w:pageBreakBefore"),
        "pages are not separated: {document}"
    );
}

#[test]
fn xlsx_export_pages_writes_one_sheet_per_page() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("pages.xlsx");
    let pages = [page_with(0, "first page"), page_with(1, "second page")];
    xlsx::export_xlsx_pages(&pages, &path).expect("export xlsx pages");

    let workbook = zip_entry(&path, "xl/workbook.xml");
    assert!(workbook.contains("Page 1"), "missing sheet 1: {workbook}");
    assert!(workbook.contains("Page 2"), "missing sheet 2: {workbook}");

    let sheet1 = "xl/worksheets/sheet1.xml";
    let sheet2 = "xl/worksheets/sheet2.xml";
    assert!(
        cell_ref_of(&path, sheet1, "first page").is_some(),
        "page 1 text missing"
    );
    assert!(
        cell_ref_of(&path, sheet2, "second page").is_some(),
        "page 2 text missing"
    );
    assert!(
        cell_ref_of(&path, sheet1, "second page").is_none(),
        "page 2 leaked into sheet 1"
    );
}

#[test]
fn xlsx_stacks_two_tables_on_one_sheet_without_colliding() {
    let table_region = |top: f32, label: &str| {
        let mut structure = reconstruct(
            BBox::new(0.0, top, 100.0, top + 20.0),
            &[BBox::new(0.0, top, 100.0, top + 20.0)],
            &[BBox::new(0.0, top, 100.0, top + 20.0)],
            &[],
        );
        structure.cells[0].text = label.to_string();
        Region {
            kind: RegionKind::Table,
            bbox: BBox::new(0.0, top, 100.0, top + 20.0),
            confidence: 0.9,
            text: None,
            table: Some(structure),
        }
    };

    let page = PageResult {
        page_index: 0,
        width: 200,
        height: 200,
        layout: PageLayout {
            width: 200,
            height: 200,
            regions: vec![
                table_region(0.0, "TOP TABLE"),
                table_region(100.0, "BOTTOM TABLE"),
            ],
        },
        text: String::new(),
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("tables.xlsx");
    xlsx::export_xlsx(&page, &path).expect("export xlsx");
    let sheet = "xl/worksheets/sheet1.xml";

    let top = cell_ref_of(&path, sheet, "TOP TABLE").expect("first table missing");
    let bottom = cell_ref_of(&path, sheet, "BOTTOM TABLE").expect("second table missing");
    assert_ne!(
        top, bottom,
        "second table overwrote the first: both in cell {top}"
    );
}
