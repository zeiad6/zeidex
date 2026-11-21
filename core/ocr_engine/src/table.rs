use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{OcrError, OcrResult};
use crate::geometry::BBox;

/// One cell of a reconstructed table. `row`/`col` are 0-based; `row_span` and
/// `col_span` are at least 1 and describe a merged cell anchored at `row`/`col`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub bbox: BBox,
    pub text: String,
}

/// A table reconstructed from a layout/table model. The grid is explicit so the
/// exporters can write native Word/Excel tables with real merged cells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableStructure {
    pub bbox: BBox,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<TableCell>,
}

impl TableStructure {
    /// The cell anchored at `(row, col)`, if it is the start of a (possibly
    /// merged) cell.
    pub fn cell_at(&self, row: usize, col: usize) -> Option<&TableCell> {
        self.cells
            .iter()
            .find(|cell| cell.row == row && cell.col == col)
    }

    /// True when `(row, col)` is covered by a merge anchored elsewhere.
    pub fn is_covered(&self, row: usize, col: usize) -> bool {
        self.cells.iter().any(|cell| {
            (cell.row_span > 1 || cell.col_span > 1)
                && row >= cell.row
                && row < cell.row + cell.row_span
                && col >= cell.col
                && col < cell.col + cell.col_span
                && !(row == cell.row && col == cell.col)
        })
    }

    /// True when `(row, col)` is a horizontal continuation of a `grid_span` merge
    /// in the same row. Word covers those columns with the anchor cell, so the
    /// exporter must not emit a separate empty cell for them.
    pub fn is_horizontal_continuation(&self, row: usize, col: usize) -> bool {
        self.cells.iter().any(|cell| {
            cell.row == row && cell.col_span > 1 && col > cell.col && col < cell.col + cell.col_span
        })
    }
}

/// Reconstructs a table from row and column rectangles.
///
/// `row_boxes` are full-width row rectangles, `col_boxes` full-height column
/// rectangles; a cell is their intersection. `merged` lists
/// `(row, col, row_span, col_span)` for cells that span more than one grid
/// position, matching SLANet's `row_span`/`col_span` output.
pub fn reconstruct(
    table_bbox: BBox,
    row_boxes: &[BBox],
    col_boxes: &[BBox],
    merged: &[(usize, usize, usize, usize)],
) -> TableStructure {
    let mut rows = row_boxes.to_vec();
    rows.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cols = col_boxes.to_vec();
    cols.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));

    let mut cells = Vec::with_capacity(rows.len() * cols.len());
    for (row, row_box) in rows.iter().enumerate() {
        for (col, col_box) in cols.iter().enumerate() {
            cells.push(TableCell {
                row,
                col,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(
                    row_box.x0.max(col_box.x0),
                    row_box.y0.max(col_box.y0),
                    row_box.x1.min(col_box.x1),
                    row_box.y1.min(col_box.y1),
                ),
                text: String::new(),
            });
        }
    }

    apply_merges(&mut cells, merged);

    TableStructure {
        bbox: table_bbox,
        rows: rows.len(),
        cols: cols.len(),
        cells,
    }
}

/// Applies merge spans and removes the cells they cover, so every grid position
/// is represented by exactly one anchor cell.
fn apply_merges(cells: &mut Vec<TableCell>, merged: &[(usize, usize, usize, usize)]) {
    for &(row, col, row_span, col_span) in merged {
        if let Some(anchor) = cells
            .iter_mut()
            .find(|cell| cell.row == row && cell.col == col)
        {
            anchor.row_span = row_span.max(1);
            anchor.col_span = col_span.max(1);
        }
    }
    cells.retain(|cell| {
        // Drop a cell that is covered by an earlier anchor's merge.
        !merged.iter().any(|&(row, col, row_span, col_span)| {
            cell.row >= row
                && cell.row < row + row_span
                && cell.col >= col
                && cell.col < col + col_span
                && !(cell.row == row && cell.col == col)
        })
    });
}

/// Deduplicates SLANet's fixed 501-candidate output: the same cell is emitted
/// several times with slightly different coordinates, so keep the highest-score
/// box in each overlap group.
pub fn nms_cells(
    cells: &[(BBox, usize, usize, f32)],
    iou_threshold: f32,
) -> Vec<(BBox, usize, usize, f32)> {
    let mut sorted: Vec<&(BBox, usize, usize, f32)> = cells.iter().collect();
    sorted.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<(BBox, usize, usize, f32)> = Vec::new();
    for cell in sorted {
        let duplicate = kept
            .iter()
            .any(|other| other.0.iou(&cell.0) > iou_threshold);
        if !duplicate {
            kept.push(*cell);
        }
    }
    kept
}

/// Reconstructs a table from SLANet-style cell predictions: each cell carries
/// its box and its `row_span`/`col_span`. Rows and columns are recovered by
/// clustering the cell top/left edges, so the model does not have to emit the
/// grid lines explicitly.
pub fn reconstruct_from_cells(cells: Vec<(BBox, usize, usize)>) -> TableStructure {
    let bbox = cells
        .iter()
        .map(|(bbox, _, _)| *bbox)
        .reduce(|acc, next| BBox {
            x0: acc.x0.min(next.x0),
            y0: acc.y0.min(next.y0),
            x1: acc.x1.max(next.x1),
            y1: acc.y1.max(next.y1),
        })
        .unwrap_or_default();

    let rows = cluster_edges(cells.iter().map(|(bbox, _, _)| bbox.y0), 8.0);
    let cols = cluster_edges(cells.iter().map(|(bbox, _, _)| bbox.x0), 8.0);
    let table_cells = cells
        .into_iter()
        .map(|(bbox, row_span, col_span)| {
            let row = edge_index(&rows, bbox.y0).unwrap_or(0);
            let col = edge_index(&cols, bbox.x0).unwrap_or(0);
            TableCell {
                row,
                col,
                row_span: row_span.max(1),
                col_span: col_span.max(1),
                bbox,
                text: String::new(),
            }
        })
        .collect();
    TableStructure {
        bbox,
        rows: rows.len(),
        cols: cols.len(),
        cells: table_cells,
    }
}

fn cluster_edges(values: impl Iterator<Item = f32>, tolerance: f32) -> Vec<f32> {
    let mut sorted: Vec<f32> = values.collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f32> = Vec::new();
    for value in sorted {
        if out
            .last()
            .is_none_or(|last| (value - *last).abs() > tolerance)
        {
            out.push(value);
        }
    }
    out
}

fn edge_index(edges: &[f32], value: f32) -> Option<usize> {
    edges.iter().position(|edge| (edge - value).abs() <= 8.0)
}

/// Assigns each recognized line to the cell it overlaps most.
///
/// The structure model emits text-sized boxes that can sit a few pixels off the
/// glyphs, most visibly for a single digit in a wide column. Requiring the
/// line's centre to fall inside a cell silently dropped those words; choosing
/// the largest overlap forgives the offset without inventing matches for lines
/// that lie outside the grid.
pub fn assign_texts(table: &mut TableStructure, words: &[(BBox, String)]) {
    for (bbox, text) in words {
        let mut best: Option<(usize, f32, f32)> = None;
        for (index, cell) in table.cells.iter().enumerate() {
            let overlap = bbox.intersection_area(&cell.bbox);
            if overlap <= 0.0 {
                continue;
            }
            let area = cell.bbox.area();
            let better = match best {
                None => true,
                Some((_, best_overlap, best_area)) => {
                    overlap > best_overlap || (overlap == best_overlap && area < best_area)
                }
            };
            if better {
                best = Some((index, overlap, area));
            }
        }
        if let Some((index, _, _)) = best {
            let cell = &mut table.cells[index];
            if !cell.text.is_empty() {
                cell.text.push(' ');
            }
            cell.text.push_str(text);
        }
    }
}

/// Rebuilds a table from the ruling lines drawn on the page.
///
/// The layout model has five classes and a table it calls a `figure` (or plain
/// `text`) is lost to the exporters. Ruling lines are the one structural signal
/// that survives without a model: a region whose dark rows span most of its
/// width, and whose dark columns span most of its height, is a grid. Returns
/// `None` unless at least a 2x2 grid is found, so a framed photograph or an
/// underlined heading cannot masquerade as a table.
pub fn from_ruling_lines(image: &image::DynamicImage, bbox: BBox) -> Option<TableStructure> {
    let (image_width, image_height) = (image.width(), image.height());
    let rect = bbox.clamp(image_width, image_height);
    let left = rect.x0.max(0.0) as u32;
    let top = rect.y0.max(0.0) as u32;
    let width = (rect.width() as u32).min(image_width.saturating_sub(left));
    let height = (rect.height() as u32).min(image_height.saturating_sub(top));
    if width < RULING_MIN_SIDE || height < RULING_MIN_SIDE {
        return None;
    }

    let crop = image.crop_imm(left, top, width, height).to_luma8();
    // One pass over the pixels: per-row and per-column dark counts. A
    // column-major scan would touch the same buffer cache-line-by-cache-line
    // twice for a 300 dpi region.
    let mut row_dark = vec![0u32; height as usize];
    let mut col_dark = vec![0u32; width as usize];
    for y in 0..height {
        let mut row = 0u32;
        for x in 0..width {
            if crop.get_pixel(x, y).0[0] < RULING_DARK {
                row += 1;
                col_dark[x as usize] += 1;
            }
        }
        row_dark[y as usize] = row;
    }

    let rows: Vec<u32> = (0..height)
        .filter(|&y| row_dark[y as usize] * 2 >= width)
        .collect();
    let cols: Vec<u32> = (0..width)
        .filter(|&x| col_dark[x as usize] * 2 >= height)
        .collect();

    let row_edges = group_runs(rows);
    let col_edges = group_runs(cols);
    // Two lines in each direction is a box, not a table.
    if row_edges.len() < 3 || col_edges.len() < 3 {
        return None;
    }

    let mut cells = Vec::with_capacity((row_edges.len() - 1) * (col_edges.len() - 1));
    for (row, rows) in row_edges.windows(2).enumerate() {
        for (col, cols) in col_edges.windows(2).enumerate() {
            cells.push(TableCell {
                row,
                col,
                row_span: 1,
                col_span: 1,
                bbox: BBox::new(
                    rect.x0 + cols[0] as f32,
                    rect.y0 + rows[0] as f32,
                    rect.x0 + cols[1] as f32,
                    rect.y0 + rows[1] as f32,
                ),
                text: String::new(),
            });
        }
    }

    Some(TableStructure {
        bbox: rect,
        rows: row_edges.len() - 1,
        cols: col_edges.len() - 1,
        cells,
    })
}

/// Collapses consecutive scan lines into one edge at the run's midpoint.
fn group_runs(lines: Vec<u32>) -> Vec<u32> {
    let mut edges: Vec<u32> = Vec::new();
    let mut run: Vec<u32> = Vec::new();
    for line in lines {
        if let Some(last) = run.last()
            && line != last + 1
        {
            edges.push(run[run.len() / 2]);
            run.clear();
        }
        run.push(line);
    }
    if !run.is_empty() {
        edges.push(run[run.len() / 2]);
    }
    edges
}

/// A ruling line must be this dark to count (0 = black).
const RULING_DARK: u8 = 160;
/// Regions smaller than this on either side are never scanned for rulings.
const RULING_MIN_SIDE: u32 = 24;

/// The structure-token class list a SLANet export was trained with
/// (PP-StructureV2's `table_structure_dict.txt`: 28 HTML fragments plus the two
/// attention tokens described below).
///
/// The shipped `en_ppstructure_mobile_v2.0_SLANet` export is built with
/// PaddleOCR's `TableLabelDecode`, which applies `merge_no_span_structure` to the
/// file (`<td>` dropped, `<td></td>` appended) and then adds the attention `sos`
/// and `eos` tokens, so the model's 30 classes are `sos`, 28 structure tokens,
/// and `eos` — in that order. The order is reproduced explicitly because an
/// off-by-one here turns `</tr>` into a cell token. Tokens are matched by
/// *text*, never by a hardcoded index, so a different export cannot silently
/// shift a semantic token into the wrong role.
#[derive(Clone, Debug)]
pub struct StructureDict {
    tokens: Vec<String>,
}

impl StructureDict {
    pub fn from_entries(entries: impl IntoIterator<Item = String>) -> Self {
        let mut tokens: Vec<String> = entries
            .into_iter()
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty() && entry != "<td>")
            .collect();
        if !tokens.iter().any(|entry| entry == "<td></td>") {
            tokens.push("<td></td>".to_string());
        }
        // `sos`/`eos` are the attention start/end classes the model was trained
        // with (`AttnLabelDecode`). Keep an already-labelled dictionary as-is so
        // a re-export cannot end up with duplicates.
        if tokens.first().map(String::as_str) != Some("sos") {
            tokens.insert(0, String::from("sos"));
        }
        if tokens.last().map(String::as_str) != Some("eos") {
            tokens.push(String::from("eos"));
        }
        Self { tokens }
    }

    pub fn load(path: impl AsRef<Path>) -> OcrResult<Self> {
        let contents = std::fs::read_to_string(path)?;
        let dict = Self::from_entries(contents.lines().map(str::to_string));
        if dict.len() < 2 {
            return Err(OcrError::Model(
                "table structure dictionary has no tokens".to_string(),
            ));
        }
        Ok(dict)
    }

    /// The PP-StructureV2 dictionary, used when a model folder predates the
    /// `table_structure_dict.txt` download. It is byte-equivalent to the file
    /// shipped in the model repository.
    pub fn builtin() -> Self {
        Self::from_entries(
            [
                "<thead>",
                "<tr>",
                "<td>",
                "</td>",
                "</tr>",
                "</thead>",
                "<tbody>",
                "</tbody>",
                "<td",
                "colspan=\"5\"",
                ">",
                "colspan=\"2\"",
                "colspan=\"3\"",
                "rowspan=\"2\"",
                "colspan=\"4\"",
                "colspan=\"6\"",
                "rowspan=\"3\"",
                "colspan=\"9\"",
                "colspan=\"10\"",
                "colspan=\"7\"",
                "rowspan=\"4\"",
                "rowspan=\"5\"",
                "rowspan=\"9\"",
                "colspan=\"8\"",
                "rowspan=\"8\"",
                "rowspan=\"6\"",
                "rowspan=\"7\"",
                "rowspan=\"10\"",
            ]
            .into_iter()
            .map(String::from),
        )
    }

    pub fn get(&self, class: i64) -> Option<&str> {
        usize::try_from(class)
            .ok()
            .and_then(|index| self.tokens.get(index))
            .map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// One cell decoded from a structure sequence, before grid placement.
struct StructureCell {
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    bbox: BBox,
}

/// Decodes a SLANet structure sequence into a positioned cell grid.
///
/// `tokens` is the per-position argmax of the structure head and `boxes` the
/// matching normalized `(x0, y0, x1, y1)` predictions in `[0, 1]` relative to
/// the table region. Only positions carrying a cell token become cells;
/// structural tokens (`<thead>`, `<tr>`, `</tr>`, `<tbody>`, ...) drive the grid
/// instead of fabricating cells from their (meaningless) boxes.
pub fn decode_structure(
    dict: &StructureDict,
    tokens: &[i64],
    boxes: &[BBox],
    region: BBox,
) -> TableStructure {
    let mut cells: Vec<StructureCell> = Vec::new();
    let mut row: usize = 0;
    let mut col: usize = 0;
    let mut row_open = false;
    // An open `<td` waiting for its span attributes and closing `>`.
    let mut pending: Option<(usize, usize)> = None;

    for (position, &class) in tokens.iter().enumerate() {
        let Some(token) = dict.get(class) else {
            continue;
        };
        let token = token.trim();
        // `eos` ends the sequence; positions after it are decoder padding and
        // must not be read as structure or cells.
        if token == "eos" {
            break;
        }
        let bbox = boxes.get(position).copied().unwrap_or_default();

        if token == "<tr>" {
            if row_open {
                row += 1;
            }
            row_open = true;
            col = 0;
            pending = None;
            continue;
        }
        if token == "<td></td>" || token == "<td>" {
            cells.push(StructureCell {
                row,
                col,
                row_span: 1,
                col_span: 1,
                bbox,
            });
            col += 1;
            pending = None;
            continue;
        }
        if token == "<td" {
            pending = Some((1, 1));
            continue;
        }
        if token == ">" {
            if let Some((row_span, col_span)) = pending.take() {
                cells.push(StructureCell {
                    row,
                    col,
                    row_span,
                    col_span,
                    bbox,
                });
                col += col_span;
            }
            continue;
        }
        if let Some((row_span, col_span)) = pending.as_mut() {
            if let Some(value) = attribute_value(token, "rowspan") {
                *row_span = value.max(1);
            }
            if let Some(value) = attribute_value(token, "colspan") {
                *col_span = value.max(1);
            }
        }
    }

    if cells.is_empty() {
        return TableStructure {
            bbox: region,
            rows: 0,
            cols: 0,
            cells: Vec::new(),
        };
    }

    let mut grid: Vec<Vec<bool>> = Vec::new();
    let mut placed: Vec<TableCell> = Vec::with_capacity(cells.len());
    for cell in cells {
        let mut col = cell.col;
        ensure_grid(&mut grid, cell.row + cell.row_span, col + cell.col_span);
        while occupied(&grid, cell.row, col) {
            col += 1;
            ensure_grid(&mut grid, cell.row + cell.row_span, col + cell.col_span);
        }
        for row_index in cell.row..cell.row + cell.row_span {
            ensure_grid(&mut grid, row_index + 1, col + cell.col_span);
            for slot in &mut grid[row_index][col..col + cell.col_span] {
                *slot = true;
            }
        }
        placed.push(TableCell {
            row: cell.row,
            col,
            row_span: cell.row_span,
            col_span: cell.col_span,
            bbox: to_page_space(cell.bbox, region),
            text: String::new(),
        });
    }

    let rows = placed
        .iter()
        .map(|cell| cell.row + cell.row_span)
        .max()
        .unwrap_or(0);
    let cols = placed
        .iter()
        .map(|cell| cell.col + cell.col_span)
        .max()
        .unwrap_or(0);

    TableStructure {
        bbox: region,
        rows,
        cols,
        cells: placed,
    }
}

/// Maps a normalized box inside `region` into page space, clamped to the region.
fn to_page_space(normalized: BBox, region: BBox) -> BBox {
    let map = |x: f32, y: f32| {
        (
            (region.x0 + x * region.width()).clamp(region.x0, region.x1),
            (region.y0 + y * region.height()).clamp(region.y0, region.y1),
        )
    };
    let (x0, y0) = map(normalized.x0, normalized.y0);
    let (x1, y1) = map(normalized.x1, normalized.y1);
    BBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

fn attribute_value(token: &str, attribute: &str) -> Option<usize> {
    let prefix = format!("{attribute}=\"");
    let rest = token.strip_prefix(&prefix)?;
    rest.strip_suffix('"')?.parse().ok()
}

fn ensure_grid(grid: &mut Vec<Vec<bool>>, rows: usize, cols: usize) {
    while grid.len() < rows {
        grid.push(Vec::new());
    }
    for row in grid.iter_mut() {
        if row.len() < cols {
            row.resize(cols, false);
        }
    }
}

fn occupied(grid: &[Vec<bool>], row: usize, col: usize) -> bool {
    grid.get(row)
        .and_then(|cells| cells.get(col))
        .copied()
        .unwrap_or(false)
}

/// Parses a numeric cell: strips currency symbols, thousands separators and
/// Arabic-Indic digits. Returns `None` when the text is not a number, so the
/// exporter writes it as text instead of a bogus 0.
pub fn parse_number(text: &str) -> Option<f64> {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.trim().chars() {
        let normalized = match ch {
            '٠'..='٩' => char::from_digit(ch as u32 - '٠' as u32, 10)?,
            '۰'..='۹' => char::from_digit(ch as u32 - '۰' as u32, 10)?,
            ',' | ' ' | '\u{00A0}' | '\u{202F}' => continue,
            '+' | '-' | '.' | '0'..='9' | 'e' | 'E' => ch,
            '$' | '€' | '£' | '¥' | '﷼' | 'د' | 'ج' | 'ر' => continue,
            _ => return None,
        };
        cleaned.push(normalized);
    }
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(y0: f32, y1: f32) -> BBox {
        BBox::new(0.0, y0, 100.0, y1)
    }

    fn col(x0: f32, x1: f32) -> BBox {
        BBox::new(x0, 0.0, x1, 100.0)
    }

    #[test]
    fn reconstruct_builds_a_grid_from_rows_and_columns() {
        let table = reconstruct(
            BBox::new(0.0, 0.0, 100.0, 100.0),
            &[row(0.0, 50.0), row(50.0, 100.0)],
            &[col(0.0, 50.0), col(50.0, 100.0)],
            &[],
        );
        assert_eq!(table.rows, 2);
        assert_eq!(table.cols, 2);
        assert_eq!(table.cells.len(), 4);
        assert_eq!(
            table.cell_at(1, 1).map(|cell| cell.bbox),
            Some(BBox::new(50.0, 50.0, 100.0, 100.0))
        );
    }

    #[test]
    fn merges_remove_covered_cells_and_set_spans() {
        let table = reconstruct(
            BBox::new(0.0, 0.0, 100.0, 100.0),
            &[row(0.0, 50.0), row(50.0, 100.0)],
            &[col(0.0, 50.0), col(50.0, 100.0)],
            &[(0, 0, 1, 2)],
        );
        assert_eq!(table.cells.len(), 3);
        let anchor = table.cell_at(0, 0).expect("anchor");
        assert_eq!(anchor.col_span, 2);
        assert!(table.cell_at(0, 1).is_none());
        assert!(table.is_covered(0, 1));
    }

    #[test]
    fn ruling_lines_rebuild_a_grid() {
        let mut canvas = image::GrayImage::from_pixel(120, 120, image::Luma([255]));
        for line in [10u32, 60, 110] {
            for pos in 0..120u32 {
                canvas.put_pixel(pos, line, image::Luma([0]));
                canvas.put_pixel(line, pos, image::Luma([0]));
            }
        }
        let image = image::DynamicImage::ImageLuma8(canvas);
        let table = super::from_ruling_lines(&image, BBox::new(0.0, 0.0, 120.0, 120.0))
            .expect("grid recovered");
        assert_eq!((table.rows, table.cols), (2, 2));
        assert_eq!(table.cells.len(), 4);
        let first = table.cell_at(0, 0).expect("first cell");
        assert!((first.bbox.x0 - 10.0).abs() < 1.0);
        assert!((first.bbox.y0 - 10.0).abs() < 1.0);
        assert!((first.bbox.x1 - 60.0).abs() < 1.0);
    }

    #[test]
    fn ruling_lines_ignore_a_blank_or_single_box_region() {
        let blank = image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            80,
            80,
            image::Luma([255]),
        ));
        assert!(super::from_ruling_lines(&blank, BBox::new(0.0, 0.0, 80.0, 80.0)).is_none());

        let mut boxed = image::GrayImage::from_pixel(80, 80, image::Luma([255]));
        for pos in 0..80u32 {
            boxed.put_pixel(pos, 10, image::Luma([0]));
            boxed.put_pixel(pos, 70, image::Luma([0]));
            boxed.put_pixel(10, pos, image::Luma([0]));
            boxed.put_pixel(70, pos, image::Luma([0]));
        }
        let boxed = image::DynamicImage::ImageLuma8(boxed);
        assert!(
            super::from_ruling_lines(&boxed, BBox::new(0.0, 0.0, 80.0, 80.0)).is_none(),
            "a frame is not a table"
        );
    }

    #[test]
    fn assign_texts_uses_overlap_not_just_the_center() {
        let mut table = reconstruct(
            BBox::new(0.0, 0.0, 100.0, 100.0),
            &[row(0.0, 100.0)],
            &[col(0.0, 50.0), col(80.0, 100.0)],
            &[],
        );
        assign_texts(
            &mut table,
            &[
                (BBox::new(10.0, 10.0, 30.0, 30.0), "left".to_string()),
                (BBox::new(60.0, 10.0, 90.0, 30.0), "right".to_string()),
                // Centre at x=76, outside both cells, but it overlaps the right
                // cell: the model's cell boxes can sit a few pixels off a glyph.
                (BBox::new(70.0, 40.0, 82.0, 60.0), "7".to_string()),
            ],
        );
        assert_eq!(
            table.cell_at(0, 0).map(|cell| cell.text.as_str()),
            Some("left")
        );
        assert_eq!(
            table.cell_at(0, 1).map(|cell| cell.text.as_str()),
            Some("right 7")
        );
    }

    #[test]
    fn parse_number_handles_currency_and_arabic_digits() {
        assert_eq!(parse_number("1,234.50"), Some(1234.5));
        assert_eq!(parse_number("$ 99.99"), Some(99.99));
        assert_eq!(parse_number("١٢٣"), Some(123.0));
        assert_eq!(parse_number("12.5%"), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn structure_dict_merges_the_nos_span_class_list() {
        // 28 base entries, `<td>` dropped, `<td></td>` appended, then the
        // attention `sos`/`eos` classes: exactly the model's 30 outputs.
        let dict = StructureDict::builtin();
        assert_eq!(dict.len(), 30);
        assert_eq!(dict.get(0), Some("sos"));
        assert_eq!(dict.get(1), Some("<thead>"));
        assert_eq!(dict.get(2), Some("<tr>"));
        assert_eq!(dict.get(28), Some("<td></td>"));
        assert_eq!(dict.get(29), Some("eos"));
    }

    #[test]
    fn decode_structure_stops_at_eos() {
        let dict = StructureDict::builtin();
        // One complete row, end token, then a cell token that must be ignored:
        // the model pads after `eos` with `sos`, but a corrupt sequence can put
        // anything there.
        let tokens = [1, 2, 28, 28, 4, 5, 29, 28, 28];
        let boxes = vec![BBox::new(0.0, 0.0, 0.1, 0.1); tokens.len()];
        let table = decode_structure(&dict, &tokens, &boxes, BBox::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(table.rows, 1);
        assert_eq!(table.cols, 2);
        assert_eq!(table.cells.len(), 2, "tokens after eos became cells");
    }

    /// The exact argmax sequence the shipped `slanet_table.onnx` emits for a
    /// 3-row × 2-column grid. Structural tokens must not become cells.
    const GRID_3X2: [i64; 17] = [1, 2, 28, 28, 4, 5, 6, 2, 28, 28, 4, 2, 28, 28, 4, 7, 29];

    #[test]
    fn decode_structure_builds_the_grid_from_tr_and_cell_tokens() {
        let dict = StructureDict::builtin();
        let boxes = vec![BBox::new(0.0, 0.0, 0.1, 0.1); GRID_3X2.len()];
        let table = decode_structure(&dict, &GRID_3X2, &boxes, BBox::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(table.rows, 3);
        assert_eq!(table.cols, 2);
        assert_eq!(table.cells.len(), 6, "structural tokens became cells");
        // Cells are anchored row by row, left to right.
        assert_eq!(table.cell_at(0, 0).map(|cell| cell.row), Some(0));
        assert_eq!(table.cell_at(2, 1).map(|cell| cell.col), Some(1));
        assert!(table.cells.iter().all(|cell| cell.row_span == 1));
        assert!(table.cells.iter().all(|cell| cell.col_span == 1));
    }

    #[test]
    fn decode_structure_handles_a_single_row_table() {
        let dict = StructureDict::builtin();
        let tokens = [1, 2, 28, 28, 4, 5, 6, 2, 28, 28, 4, 7, 29];
        let boxes = vec![BBox::default(); tokens.len()];
        let table = decode_structure(&dict, &tokens, &boxes, BBox::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!((table.rows, table.cols), (2, 2));
        assert_eq!(table.cells.len(), 4);
    }

    #[test]
    fn decode_structure_reads_spans_when_the_export_emits_them() {
        // `<tr> <td colspan="2" > <td></td> </tr>`: one spanned cell plus one
        // plain cell must occupy three grid columns.
        let dict = StructureDict::builtin();
        let tokens = [2, 8, 11, 10, 28, 4];
        let boxes = vec![BBox::default(); tokens.len()];
        let table = decode_structure(&dict, &tokens, &boxes, BBox::new(0.0, 0.0, 300.0, 100.0));
        assert_eq!((table.rows, table.cols), (1, 3));
        assert_eq!(table.cells.len(), 2);
        let anchor = table.cell_at(0, 0).expect("spanned anchor");
        assert_eq!((anchor.row_span, anchor.col_span), (1, 2));
        // The following cell is pushed past the merged columns.
        assert!(table.cell_at(0, 2).is_some());
        assert!(table.is_covered(0, 1));
    }

    #[test]
    fn decode_structure_maps_normalized_boxes_into_page_space() {
        let dict = StructureDict::builtin();
        // Position 0 is `<tr>`; the cell box lives at the cell token's position.
        let tokens = [2, 28];
        let boxes = vec![BBox::default(), BBox::new(0.25, 0.5, 0.75, 1.0)];
        let table = decode_structure(
            &dict,
            &tokens,
            &boxes,
            BBox::new(100.0, 200.0, 500.0, 400.0),
        );
        let cell = table.cell_at(0, 0).expect("cell");
        assert!((cell.bbox.x0 - 200.0).abs() < 1e-4);
        assert!((cell.bbox.y0 - 300.0).abs() < 1e-4);
        assert!((cell.bbox.x1 - 400.0).abs() < 1e-4);
        assert!((cell.bbox.y1 - 400.0).abs() < 1e-4);
    }

    #[test]
    fn decode_structure_returns_an_empty_table_without_cells() {
        let dict = StructureDict::builtin();
        let tokens = [1, 2, 4, 5, 6, 7];
        let boxes = vec![BBox::default(); tokens.len()];
        let table = decode_structure(&dict, &tokens, &boxes, BBox::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!((table.rows, table.cols), (0, 0));
        assert!(table.cells.is_empty());
    }

    #[test]
    fn nms_cells_keeps_the_highest_score_per_overlap_group() {
        let cells = vec![
            (BBox::new(0.0, 0.0, 10.0, 10.0), 1, 1, 0.9),
            (BBox::new(1.0, 1.0, 11.0, 11.0), 1, 1, 0.8),
            (BBox::new(50.0, 50.0, 60.0, 60.0), 1, 1, 0.7),
        ];
        let kept = super::nms_cells(&cells, 0.5);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].3, 0.9);
    }
}
