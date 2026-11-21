# OCR Studio

The OCR Studio is a separate surface from the launcher, opened from the
**OCR Studio** button (or the tray menu). Where the old macOS shell searched
*inside* every indexed image and PDF, the Studio is a single-document workbench:
you open one file, read it on-device, correct what it got wrong, and export it.

```text
Open file ─▶ OCR ─▶ page preview + editable regions ─▶ .docx / .xlsx
```

## Opening a document

The file dialog accepts `png`, `jpg`, `jpeg`, `tif`, `tiff`, `bmp`, `webp` and
`pdf`. An image becomes one page. A PDF is rendered with pdfium at 300 dpi, one
bounded chunk at a time, so a long document does not hold every bitmap in
memory. Detection and recognition for a chunk run in parallel on rayon; the
Studio follows along through `ocr-progress` events.

## The two panes

**Left — the page.** `ocr_page_preview` renders the page to a PNG data URL, so
the image shows up without enabling an asset-protocol scope. Region boxes are
drawn over it in page coordinates; hovering shows the kind and confidence, and
clicking a box selects that region on the right. The **Regions** checkbox hides
the overlay.

**Right — the text**, in one of two modes:

| Mode | Behaviour |
| --- | --- |
| *Regions* | One editor per extracted region. Table regions render a real cell grid: clicking a cell edits that cell, and merged cells keep their span. Edits go straight into the page's region structure. |
| *Text* | The whole page as plain text, in reading order. Paragraphs are handed back to the text regions when you leave the field or export. |

Both modes write into `PageResult.layout.regions`, which is what the exporters
read — an edit can no longer be lost between the textarea and the file.

## Reading order

The pipeline (`core/ocr_engine`) runs:

1. **Layout** — PP-StructureV2 (PicoDet) finds regions: text, title, list,
   table, figure, equation, header, footer, caption.
2. **Text lines** — DBNet detects each line; the box is expanded with
   PaddleOCR's `unclip` ratio so ascenders and descenders survive the crop.
3. **Recognition** — PP-OCRv4, English and Arabic. Both models score every
   line; an Arabic-script result above 0.5 confidence wins outright because the
   two mean log-probabilities are not comparable across character sets.
4. **Tables** — SLANet decodes its structure tokens, so `<tr>`/`<td>` drive the
   grid instead of every one of the 501 positions becoming a cell; recognized
   text is then assigned to the cells.
5. **Order** — regions are sorted top-to-bottom and, on a page whose text is
   Arabic, right-to-left.

## Exporting

| Target | What it keeps |
| --- | --- |
| `.docx` | Real Word document; `w:bidi` on Arabic paragraphs **and table cells**, native tables with merges, bold titles, a page break between pages |
| `.xlsx` | Real spreadsheet; one worksheet per page, RTL sheet view, merged cells, numeric cells kept as numbers, tables stacked with a row cursor instead of overwriting each other |

Export writes **all pages** currently loaded (`export_docx_pages` /
`export_xlsx_pages`), not only the active one. The `PageResult`s travel to Rust
as JSON, so what you edited is what gets written.

## Models

The engine is offline-only. `make models` (or `scripts/fetch_models.sh`)
downloads five ONNX models plus their dictionaries and metadata into
`third_party/models`, and writes `SHA256SUMS` for verification:

```text
ppocrv4_det.onnx            ppstructurev2_layout.onnx   infer_cfg.yml
ppocrv4_rec_en.onnx         slanet_table.onnx           table_structure_dict.txt
ppocrv4_rec_ar.onnx         ppocr_keys_v1.txt           arabic_dict.txt
```

At runtime the Studio reports which files are missing instead of failing
mid-run; `ZEIDEX_MODELS_DIR` overrides the location, then the bundled resources
are searched, then a development checkout.

The ONNX Runtime and pdfium shared libraries are loaded dynamically. Drop them
in `src-tauri/resources/lib/` before packaging (the bundler copies them to
`<resources>/lib`), or point `ORT_DYLIB_PATH` / `ZEIDEX_PDFIUM_LIBRARY` at them.
`cuda` / `directml` cargo features are opt-in; the default is CPU.

## Limits

- **Arabic accuracy is model-bound.** `arabic_PP-OCRv4_rec` reads shaped Arabic
  in logical order correctly, but loses characters as a line gets denser. On
  synthetic Noto Kufi renders at 40 px it is exact for
  `مرحبا بكم في لوميو` and `كتاب الطالب` (0.82–0.88 confidence) and drops
  Arabic-Indic digits (`١٢٣٤٥` → `٢٣٥`). This is a model limit, not a pipeline
  bug; replacing `ppocrv4_rec_ar.onnx` and `arabic_dict.txt` is the upgrade
  path, and the editor exists because of it.
- Very small text (below ~20 px) degrades both scripts, since recognition
  normalizes every line to 48 px tall.
- Layout is only as good as PicoDet's five classes. When the layout model does
  not call a grid a `table`, the pipeline still scans the region for ruling
  lines: three or more rows and columns of dark lines become a real grid, and a
  cell with no recognized text leaves the region as plain text. A table drawn
  without ruling lines (whitespace-separated columns) is not recovered.
