use std::fs::File;
use std::path::Path;

use docx_rs::{
    AlignmentType, BorderType, Document, Docx, Paragraph, Run, Table, TableBorder,
    TableBorderPosition, TableBorders, TableCell, TableRow, VMergeType,
};

use zeidex_ocr::PageResult;
use zeidex_ocr::bidi;
use zeidex_ocr::layout::RegionKind;
use zeidex_ocr::table::TableStructure;

use crate::error::ExportResult;

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
                    document = document.add_table(*table);
                }
            }
        }
        if break_pending {
            document = document.add_paragraph(Paragraph::new().page_break_before(true));
        }
    }

    let docx = Docx::new().document(document);
    let mut file = File::create(path)?;
    docx.build().pack(&mut file)?;
    Ok(())
}

/// Both variants are large and each block is moved exactly once, so boxing them
/// keeps the enum (and the `Vec<Block>` it lives in) small for free.
enum Block {
    Paragraph(Box<Paragraph>),
    Table(Box<Table>),
}

fn page_blocks(page: &PageResult) -> Vec<Block> {
    let mut blocks = Vec::new();
    for region in &page.layout.regions {
        match region.kind {
            RegionKind::Table => {
                if let Some(table) = &region.table {
                    blocks.push(Block::Table(Box::new(table_to_docx(table))));
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
