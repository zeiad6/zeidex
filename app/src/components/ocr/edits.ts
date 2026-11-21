import type { PageResult, Region, TableCell, TableStructure } from "../../api";

/**
 * Mirrors the Rust page flattening (`pipeline.rs`) so the plain-text view stays
 * consistent with the exporters after an edit.
 */
export function flattenPage(page: PageResult): string {
  const blocks: string[] = [];
  for (const region of page.layout.regions) {
    if (region.kind === "table") {
      const table = region.table;
      if (!table) continue;
      const rows: string[] = [];
      for (let row = 0; row < table.rows; row += 1) {
        const cells: string[] = [];
        for (let col = 0; col < table.cols; col += 1) {
          const cell = table.cells.find(
            (candidate) => candidate.row === row && candidate.col === col,
          );
          cells.push(cell?.text ?? "");
        }
        if (cells.some((cell) => cell.trim() !== "")) rows.push(cells.join("\t"));
      }
      if (rows.length > 0) blocks.push(rows.join("\n"));
      continue;
    }
    const text = region.text ?? "";
    if (text.trim() !== "") blocks.push(text);
  }
  return blocks.join("\n\n");
}

/** Replaces one page in the list without mutating the others. */
export function replacePage(pages: PageResult[], index: number, page: PageResult): PageResult[] {
  return pages.map((candidate, position) => (position === index ? page : candidate));
}

/**
 * The plain-text draft used by the text editor.
 *
 * Tables are excluded on purpose: a table's text lives in its cell grid, which
 * has its own editor, and handing flattened `row\tcell` lines back to
 * `replacePageText` would duplicate them into a text region and lose the grid.
 */
export function flattenTextRegions(page: PageResult): string {
  return page.layout.regions
    .filter((region) => region.kind !== "table")
    .map((region) => region.text ?? "")
    .filter((text) => text.trim() !== "")
    .join("\n\n");
}

/** Applies a region edit and refreshes the page's flat text. */
function withRegions(page: PageResult, regions: Region[]): PageResult {
  const updated: PageResult = {
    ...page,
    layout: { ...page.layout, regions },
  };
  return { ...updated, text: flattenPage(updated) };
}

export function updateRegionText(page: PageResult, index: number, text: string): PageResult {
  const regions = page.layout.regions.map((region, position) =>
    position === index ? { ...region, text } : region,
  );
  return withRegions(page, regions);
}

export function updateCellText(
  page: PageResult,
  regionIndex: number,
  cellIndex: number,
  text: string,
): PageResult {
  const regions = page.layout.regions.map((region, position) => {
    if (position !== regionIndex || !region.table) return region;
    const cells = region.table.cells.map((cell, cellPosition) =>
      cellPosition === cellIndex ? { ...cell, text } : cell,
    );
    const table: TableStructure = { ...region.table, cells };
    return { ...region, table };
  });
  return withRegions(page, regions);
}

/**
 * Bulk edit of the flat text.
 *
 * Paragraphs are handed back to the editable text regions in order. When the
 * count no longer matches (the user added or merged a paragraph) the remaining
 * text stays on the last region, which is the only lossless option without a
 * document model: a region is the unit the exporters read, so text that belongs
 * to no region would be dropped.
 */
export function replacePageText(page: PageResult, text: string): PageResult {
  const editable: number[] = [];
  page.layout.regions.forEach((region, index) => {
    if (region.kind !== "table") editable.push(index);
  });
  if (editable.length === 0) return { ...page, text };

  const paragraphs = text.split(/\n{2,}/);
  const assigned = new Map<number, string>();
  editable.forEach((regionIndex, position) => {
    if (position === editable.length - 1) {
      assigned.set(regionIndex, paragraphs.slice(position).join("\n\n"));
    } else {
      assigned.set(regionIndex, paragraphs[position] ?? "");
    }
  });

  const regions = page.layout.regions.map((region, index) =>
    assigned.has(index) ? { ...region, text: assigned.get(index) ?? "" } : region,
  );
  return withRegions(page, regions);
}

export type GridSlot =
  | { kind: "anchor"; cell: TableCell; index: number }
  | { kind: "covered" }
  | { kind: "empty" };

/**
 * Expands a cell span list into a dense `rows x cols` grid of slots.
 *
 * Word and Excel both need every position accounted for: an anchor carries the
 * span, the positions it covers must be skipped, and gaps stay empty.
 */
export function tableGrid(table: TableStructure): GridSlot[][] {
  const anchors = new Map<string, { cell: TableCell; index: number }>();
  table.cells.forEach((cell, index) => {
    anchors.set(`${cell.row}:${cell.col}`, { cell, index });
  });

  const grid: GridSlot[][] = [];
  for (let row = 0; row < table.rows; row += 1) {
    const line: GridSlot[] = [];
    for (let col = 0; col < table.cols; col += 1) {
      const anchor = anchors.get(`${row}:${col}`);
      if (anchor) {
        line.push({ kind: "anchor", cell: anchor.cell, index: anchor.index });
        continue;
      }
      const covered = table.cells.some(
        (cell) =>
          row >= cell.row &&
          row < cell.row + cell.row_span &&
          col >= cell.col &&
          col < cell.col + cell.col_span,
      );
      line.push(covered ? { kind: "covered" } : { kind: "empty" });
    }
    grid.push(line);
  }
  return grid;
}

/** Bounding box of a region as CSS percentages inside the page preview. */
export function regionStyle(
  region: Region,
  page: { width: number; height: number },
): Record<string, string> {
  const { x0, y0, x1, y1 } = region.bbox;
  const width = Math.max(page.width, 1);
  const height = Math.max(page.height, 1);
  return {
    left: `${(x0 / width) * 100}%`,
    top: `${(y0 / height) * 100}%`,
    width: `${(Math.abs(x1 - x0) / width) * 100}%`,
    height: `${(Math.abs(y1 - y0) / height) * 100}%`,
  };
}
