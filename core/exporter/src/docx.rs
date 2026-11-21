use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use docx_rs::{
    AlignmentType, BorderType, Document, Docx, Paragraph, Run, Table, TableBorder,
    TableBorderPosition, TableBorders, TableCell, TableRow, VMergeType,
};

use zeidex_ocr::PageResult;
use zeidex_ocr::bidi;
use zeidex_ocr::layout::RegionKind;
use zeidex_ocr::table::TableStructure;

use crate::error::{ExportError, ExportResult};

/// Writes a single page as an editable `.docx`.
pub fn export_docx(page: &PageResult, path: impl AsRef<Path>) -> ExportResult<()> {
    export_docx_pages(std::slice::from_ref(page), path)
}

/// Writes every page into one document, separated by page breaks.
///
/// Arabic paragraphs get `w:bidi` (the property Word uses for paragraph
/// direction) plus right alignment, and Arabic table cells get the same
/// treatment: a merged or plain cell whose text is Arabic must read right-to-left
/// or the export is unreadable. Text stays in logical order, so copy/paste and
/// search remain correct.
pub fn export_docx_pages(pages: &[PageResult], path: impl AsRef<Path>) -> ExportResult<()> {
    let mut document = Document::new();
    // One flag per table, in the order the tables are added to the document.
    let mut rtl_tables: Vec<bool> = Vec::new();

    for (index, page) in pages.iter().enumerate() {
        let mut break_pending = index > 0;
        for block in page_blocks(page) {
            match block {
                Block::Paragraph(mut paragraph) => {
                    if break_pending {
                        *paragraph = paragraph.page_break_before(true);
                        break_pending = false;
                    }
                    document = document.add_paragraph(*paragraph);
                }
                Block::Table(table) => {
                    // A table cannot carry a page break, so a minimal paragraph
                    // opens the page before the table is emitted.
                    if break_pending {
                        document = document.add_paragraph(Paragraph::new().page_break_before(true));
                        break_pending = false;
                    }
                    rtl_tables.push(table.direction_is_rtl);
                    document = document.add_table(table.table);
                }
            }
        }
        if break_pending {
            document = document.add_paragraph(Paragraph::new().page_break_before(true));
        }
    }

    // Packed in memory rather than straight to the file: the table direction
    // flag has no `docx-rs` setter, so it is injected into the package before
    // the bytes reach the disk. Nothing is written if that step fails.
    let mut package = Cursor::new(Vec::new());
    Docx::new().document(document).build().pack(&mut package)?;

    let mut bytes = package.into_inner();
    mark_rtl_tables(&mut bytes, &rtl_tables)?;

    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

/// The flag `docx-rs` 0.4 cannot set, written into the packed package.
///
/// Word lays table columns out left-to-right unless `w:tblPr/w:bidiVisual` is
/// present, even when every cell is right-to-left — an Arabic table came out
/// mirrored. `docx-rs` exposes no setter, so the element is injected into
/// `word/document.xml` after packing. The tables appear there in the order they
/// were added, which is what `rtl_tables` records.
///
/// The whole package is rewritten in memory and replaces the file only if every
/// step succeeds, so a failure cannot leave a half-written document.
fn mark_rtl_tables(package: &mut Vec<u8>, rtl_tables: &[bool]) -> ExportResult<()> {
    if !rtl_tables.iter().any(|rtl| *rtl) {
        return Ok(());
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(package.as_slice()))?;
    let mut document_xml = String::new();
    archive
        .by_name(DOCUMENT_XML)?
        .read_to_string(&mut document_xml)?;

    let Some(rewritten) = inject_table_direction(&document_xml, rtl_tables) else {
        return Err(ExportError::Invalid(
            "the generated document did not contain the expected tables".to_string(),
        ));
    };

    let mut rewritten_package = Cursor::new(Vec::with_capacity(package.len() + 64));
    {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut writer = zip::ZipWriter::new(&mut rewritten_package);
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            writer.start_file(entry.name().to_string(), options)?;
            if entry.name() == DOCUMENT_XML {
                writer.write_all(rewritten.as_bytes())?;
            } else {
                std::io::copy(&mut entry, &mut writer)?;
            }
        }
        writer.finish()?;
    }

    *package = rewritten_package.into_inner();
    Ok(())
}

const DOCUMENT_XML: &str = "word/document.xml";
const TABLE_PROPERTIES: &str = "<w:tblPr>";
const TABLE_DIRECTION: &str = "<w:bidiVisual />";

/// Inserts `w:bidiVisual` into the property block of each table flagged
/// right-to-left, leaving every other table untouched.
///
/// Returns `None` when the package does not hold exactly one property block per
/// table, which would mean the document and the flags disagree. Guessing then
/// would put the flag on the wrong table.
fn inject_table_direction(document_xml: &str, rtl_tables: &[bool]) -> Option<String> {
    if document_xml.matches(TABLE_PROPERTIES).count() != rtl_tables.len() {
        return None;
    }

    let mut out = String::with_capacity(document_xml.len() + 32);
    let mut rest = document_xml;
    for &rtl in rtl_tables {
        let position = rest.find(TABLE_PROPERTIES)?;
        let (head, tail) = rest.split_at(position + TABLE_PROPERTIES.len());
        out.push_str(head);
        // `bidiVisual` must precede `tblW` in the schema, and the next element
        // docx-rs writes is `tblW`, so the front of the block is the one place
        // the element can go.
        if rtl && !tail.starts_with(TABLE_DIRECTION) {
            out.push_str(TABLE_DIRECTION);
        }
        rest = tail;
    }
    out.push_str(rest);
    Some(out)
}

/// Both variants are large and each block is moved exactly once, so boxing them
/// keeps the enum (and the `Vec<Block>` it lives in) small for free.
enum Block {
    Paragraph(Box<Paragraph>),
    Table(Box<TableBlock>),
}

/// A table plus the direction it has to be written in. `docx-rs` 0.4 has no
/// setter for `w:bidiVisual`, so the flag travels beside the table and is
/// applied to the packed package.
struct TableBlock {
    table: Table,
    direction_is_rtl: bool,
}

fn page_blocks(page: &PageResult) -> Vec<Block> {
    let mut blocks = Vec::new();
    for region in &page.layout.regions {
        match region.kind {
            RegionKind::Table => {
                if let Some(table) = &region.table {
                    blocks.push(Block::Table(Box::new(TableBlock {
                        table: table_to_docx(table),
                        direction_is_rtl: table_direction_is_rtl(table),
                    })));
                }
            }
            _ => {
                let text = region.text.as_deref().unwrap_or_default();
                if !text.trim().is_empty() {
                    blocks.push(Block::Paragraph(Box::new(text_paragraph(
                        region.kind,
                        text,
                    ))));
                }
            }
        }
    }
    blocks
}

/// Whether the table reads right-to-left, decided by the words in its cells.
///
/// Only cells that carry letters vote: a numeric column is neutral and must not
/// outvote the Arabic columns beside it, and one stray Arabic word in an English
/// table must not flip the whole table. Ties stay left-to-right, the default.
fn table_direction_is_rtl(table: &TableStructure) -> bool {
    let mut rtl = 0usize;
    let mut ltr = 0usize;
    for cell in &table.cells {
        if !cell.text.chars().any(char::is_alphabetic) {
            continue;
        }
        if bidi::is_rtl(&cell.text) {
            rtl += 1;
        } else {
            ltr += 1;
        }
    }
    rtl > ltr
}

/// A body paragraph. Titles are bold and larger rather than referencing a style
/// the default template may not define.
fn text_paragraph(kind: RegionKind, text: &str) -> Paragraph {
    let rtl = bidi::is_rtl(text);
    let mut run = Run::new().add_text(text);
    let mut paragraph = Paragraph::new();
    if matches!(kind, RegionKind::Title) {
        run = run.bold().size(32);
    }
    if rtl {
        paragraph.property = paragraph.property.bidi(true);
        paragraph = paragraph.align(AlignmentType::Right);
    }
    paragraph.add_run(run)
}

fn table_to_docx(table: &TableStructure) -> Table {
    let mut rows = Vec::with_capacity(table.rows);
    for row in 0..table.rows {
        let mut cells = Vec::with_capacity(table.cols);
        for col in 0..table.cols {
            if let Some(cell) = table.cell_at(row, col) {
                let mut table_cell = TableCell::new().add_paragraph(cell_paragraph(&cell.text));
                if cell.row_span > 1 {
                    table_cell = table_cell.vertical_merge(VMergeType::Restart);
                }
                if cell.col_span > 1 {
                    table_cell = table_cell.grid_span(cell.col_span);
                }
                cells.push(table_cell);
            } else if table.is_horizontal_continuation(row, col) {
                // Word's `grid_span` covers these columns; adding an empty cell
                // would insert an extra grid column and break the merge.
                continue;
            } else if table.is_covered(row, col) {
                cells.push(TableCell::new().vertical_merge(VMergeType::Continue));
            } else {
                cells.push(TableCell::new());
            }
        }
        rows.push(TableRow::new(cells));
    }
    Table::new(rows).set_borders(table_borders())
}

/// A table cell paragraph, right-to-left when its own text is Arabic.
fn cell_paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::new();
    if bidi::is_rtl(text) {
        paragraph.property = paragraph.property.bidi(true);
        paragraph = paragraph.align(AlignmentType::Right);
    }
    paragraph.add_run(Run::new().add_text(text.to_string()))
}

fn table_borders() -> TableBorders {
    let border = |position| {
        TableBorder::new(position)
            .border_type(BorderType::Single)
            .size(4)
            .color("000000")
    };
    TableBorders::new()
        .set(border(TableBorderPosition::Top))
        .set(border(TableBorderPosition::Bottom))
        .set(border(TableBorderPosition::Left))
        .set(border(TableBorderPosition::Right))
        .set(border(TableBorderPosition::InsideH))
        .set(border(TableBorderPosition::InsideV))
}
