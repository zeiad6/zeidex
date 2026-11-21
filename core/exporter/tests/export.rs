//! Round-trip tests: build a page layout, export it, then open the resulting
//! Office package and assert the XML carries the structure we promised.

// Tests are allowed to fail loudly: the crate deny-list applies to shipped code.
#![allow(clippy::expect_used)]

use std::io::Read;
use std::path::Path;

use zeidex_exporter::{docx, xlsx};
use zeidex_ocr::geometry::BBox;
use zeidex_ocr::layout::{PageLayout, Region, RegionKind};
use zeidex_ocr::pipeline::{OcrEngine, OcrEngineConfig, PageResult};
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

fn page_with_table(title: &str, first_cell: &str, second_cell: &str) -> PageResult {
    let row = BBox::new(0.0, 0.0, 100.0, 20.0);
    let col = |x0: f32, x1: f32| BBox::new(x0, 0.0, x1, 20.0);
    let mut structure = reconstruct(
        BBox::new(0.0, 0.0, 100.0, 20.0),
        &[row, row],
        &[col(0.0, 50.0), col(50.0, 100.0)],
        &[(0, 0, 1, 2)],
    );
    structure.cells[0].text = first_cell.to_string();
    structure.cells[1].text = second_cell.to_string();

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
                    text: Some(title.to_string()),
                    table: None,
                },
                Region {
                    kind: RegionKind::Table,
                    bbox: BBox::new(0.0, 20.0, 100.0, 40.0),
                    confidence: 0.99,
                    text: None,
                    table: Some(structure),
                },
            ],
        },
        text: String::new(),
    }
}

#[test]
fn docx_marks_an_arabic_table_right_to_left() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rtl-table.docx");
    let page = page_with_table("تقرير المبيعات", "الاسم", "1,234.50");
    docx::export_docx(&page, &path).expect("export docx");

    let document = zip_entry(&path, "word/document.xml");
    // `bidiVisual` is the property that makes Word lay the columns out
    // right-to-left; without it an Arabic table exports mirrored.
    assert!(
        document.contains("<w:tblPr><w:bidiVisual />"),
        "table is not marked right-to-left: {document}"
    );
}

#[test]
fn docx_leaves_a_latin_table_left_to_right() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("ltr-table.docx");
    let page = page_with_table("Quarterly report", "Item", "1,234.50");
    docx::export_docx(&page, &path).expect("export docx");

    let document = zip_entry(&path, "word/document.xml");
    assert!(
        !document.contains("<w:bidiVisual"),
        "a Latin table must keep Word's default column order: {document}"
    );
}

#[test]
fn docx_tie_keeps_the_default_direction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mixed-table.docx");
    // One English heading and one Arabic label: a tie must stay left-to-right,
    // and the Arabic cell itself still reads right-to-left.
    let page = page_with_table("Report", "Item", "الصنف");
    docx::export_docx(&page, &path).expect("export docx");

    let document = zip_entry(&path, "word/document.xml");
    assert!(
        !document.contains("<w:bidiVisual"),
        "a tied table must keep Word's default column order: {document}"
    );
    assert!(
        document.contains("الصنف"),
        "the Arabic cell text must survive: {document}"
    );
}

#[test]
fn docx_rewrite_keeps_every_package_entry_readable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("intact.docx");
    docx::export_docx(&page(), &path).expect("export docx");

    // The table-direction pass rewrites the whole zip, so the other parts have
    // to come through unchanged and still be readable.
    for entry in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/document.xml",
        "word/styles.xml",
    ] {
        assert!(
            !zip_entry(&path, entry).is_empty(),
            "package entry {entry} is missing or empty after the rewrite"
        );
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

/// The whole real-model path: OCR the table fixture, then export the page that
/// came out of the engine. This is the join the unit tests above cannot see —
/// a page can be exported correctly and still be empty because recognition
/// filled nothing.
#[test]
fn ocr_output_exports_to_word_and_excel() {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return;
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_IMAGE not set");
        return;
    };
    if Path::new(&image_path)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("table.png")
    {
        eprintln!("skipping: fixture is not table.png");
        return;
    }

    let engine = OcrEngine::new(OcrEngineConfig {
        models_dir: models_dir.into(),
        ..OcrEngineConfig::default()
    })
    .expect("load OCR models");
    let page = engine
        .process_image(Path::new(&image_path))
        .expect("OCR the fixture");

    let table = page
        .layout
        .regions
        .iter()
        .find(|region| region.kind == RegionKind::Table)
        .expect("layout has a table region");
    let structure = table.table.as_ref().expect("table region has a grid");
    assert!(
        structure
            .cells
            .iter()
            .any(|cell| !cell.text.trim().is_empty()),
        "OCR produced a table with no cell text"
    );

    let dir = tempfile::tempdir().expect("tempdir");

    let docx_path = dir.path().join("ocr.docx");
    docx::export_docx(&page, &docx_path).expect("export docx");
    let body = zip_entry(&docx_path, "word/document.xml");
    assert!(body.contains("<w:tbl>"), "Word export has no table");
    assert!(
        body.contains("Quantity"),
        "Word export lost recognized text: {body}"
    );

    let xlsx_path = dir.path().join("ocr.xlsx");
    xlsx::export_xlsx(&page, &xlsx_path).expect("export xlsx");
    let sheet = "xl/worksheets/sheet1.xml";
    let quantity = cell_ref_of(&xlsx_path, sheet, "Quantity");
    assert!(
        quantity.is_some(),
        "Excel export lost the recognized header; shared strings were not written"
    );
    assert!(
        shared_index(&xlsx_path, "Gadget C").is_some()
            || shared_index(&xlsx_path, "Widget A").is_some(),
        "Excel export lost recognized body cells"
    );
}
