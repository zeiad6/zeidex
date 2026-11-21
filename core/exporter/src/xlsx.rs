use std::path::Path;

use rust_xlsxwriter::{Format, Workbook, Worksheet};

use zeidex_ocr::PageResult;
use zeidex_ocr::bidi;
use zeidex_ocr::layout::RegionKind;
use zeidex_ocr::table;

use crate::error::ExportResult;

/// Writes a single page as an editable `.xlsx`.
pub fn export_xlsx(page: &PageResult, path: impl AsRef<Path>) -> ExportResult<()> {
    export_xlsx_pages(std::slice::from_ref(page), path)
}

/// Writes every page as its own worksheet, in reading order.
///
/// One sheet per page avoids the collision a single sheet would have: tables on
/// different pages would otherwise both start at row 0 and overwrite each other.
pub fn export_xlsx_pages(pages: &[PageResult], path: impl AsRef<Path>) -> ExportResult<()> {
    let mut workbook = Workbook::new();
    if pages.is_empty() {
        workbook.add_worksheet();
    }
    for page in pages {
        write_page(&mut workbook, page)?;
    }
    workbook.save(path)?;
    Ok(())
}

/// Writes one page's regions into a fresh worksheet.
///
/// The sheet is named after the document's page index, not its position in the
/// slice: after a page-window run, the first loaded page may be page 5 of the
/// file and a sheet called "Page 1" would be wrong.
fn write_page(workbook: &mut Workbook, page: &PageResult) -> ExportResult<()> {
    let worksheet = workbook.add_worksheet();
    worksheet.set_name(format!("Page {}", page.page_index + 1))?;
    worksheet.set_right_to_left(is_rtl_page(page));

    // Row cursor for this sheet. Tables are stacked with a blank separator row so
    // two tables on one page cannot land on top of each other.
    let mut cursor: u32 = 0;
    for region in &page.layout.regions {
        match region.kind {
            RegionKind::Table => {
                if let Some(structure) = &region.table {
                    cursor = write_table(worksheet, structure, cursor)?;
                }
            }
            _ => {
                if let Some(text) = &region.text
                    && !text.trim().is_empty()
                {
                    worksheet.write_string(cursor, 0, text)?;
                    cursor += 1;
                }
            }
        }
    }

    worksheet.autofit();
    Ok(())
}

/// Writes a reconstructed table at `top` and returns the next free row.
///
/// Merged cells are created first with an empty string (the only way the library
/// allows a merge to carry a number), then the anchor is overwritten with the
/// typed value. A cell that parses as a number is stored as a number so Excel can
/// sum or sort it.
fn write_table(
    worksheet: &mut Worksheet,
    structure: &table::TableStructure,
    top: u32,
) -> ExportResult<u32> {
    if structure.cells.is_empty() {
        return Ok(top);
    }
    let format = Format::new();

    for cell in &structure.cells {
        let row = top.saturating_add(cell.row as u32);
        let col = cell.col as u16;
        let last_row = row.saturating_add(cell.row_span as u32 - 1);
        let last_col = col.saturating_add(cell.col_span as u16 - 1);
        let merged = cell.row_span > 1 || cell.col_span > 1;

        if merged {
            worksheet.merge_range(row, col, last_row, last_col, "", &format)?;
        }

        if cell.text.trim().is_empty() {
            // The merge itself still has to exist even when the cell is blank.
            continue;
        }
        match table::parse_number(&cell.text) {
            Some(number) if merged => {
                worksheet.write_number_with_format(row, col, number, &format)?;
            }
            Some(number) => {
                worksheet.write_number(row, col, number)?;
            }
            None if merged => {
                worksheet.write_string_with_format(row, col, &cell.text, &format)?;
            }
            None => {
                worksheet.write_string(row, col, &cell.text)?;
            }
        }
    }

    Ok(top.saturating_add(structure.rows as u32).saturating_add(1))
}

/// True when the page reads right-to-left: either a text region is RTL or a
/// table cell contains Arabic.
fn is_rtl_page(page: &PageResult) -> bool {
    page.layout.regions.iter().any(|region| {
        region.text.as_deref().is_some_and(bidi::is_rtl)
            || region.table.as_ref().is_some_and(|structure| {
                structure
                    .cells
                    .iter()
                    .any(|cell| bidi::has_arabic(&cell.text))
            })
    })
}
