# OCR Studio

The OCR Studio is a separate surface from the launcher, opened from the
**OCR Studio** button (or the tray menu). Where the old macOS shell searched
*inside* every indexed image and PDF, the Studio is a single-document workbench:
you open one file, read it on-device, correct what it got wrong, and export it.

```text
Open file ─▶ OCR ─▶ page preview + editable regions ─▶ .docx / .xlsx
```

## Opening a document

Opening a document has two doors: **Open file** and drag-and-drop onto the
window. Both accept `png`, `jpg`, `jpeg`, `tif`, `tiff`, `bmp`, `webp` and `pdf`,
both go through the same adoption path, and a dropped file with any other
extension is refused by name instead of failing later inside the engine. Only
one document can be loaded at a time, and neither door opens while a
recognition run is in flight: the old file's results would otherwise land in the
new file's panes. An image becomes one page, decoded with its EXIF or TIFF
orientation applied — a phone stores the sensor frame sideways and records how
to turn it upright in the tag, which `image::open` ignores. Preview and
detection share that loader, so a portrait photo is read upright and its region
boxes land on the pixels shown. A PDF is rendered with pdfium at 300 dpi, one
bounded chunk at a time, so a long document does not hold every bitmap in
memory. Detection and recognition for a chunk run in parallel on rayon; the
Studio follows along through `ocr-progress` events. Because a chunk finishes
pages in whatever order rayon schedules them, the bar counts completed pages
instead of trusting the last page number it was told about.

For a long PDF, the two boxes beside **Run OCR** take a 1-based, inclusive page
window; both empty means every page. A window validates against the real page
count before rendering — a first page past the end is an error, a last page past
the end is clamped — and `PageResult::page_index` keeps the document's own page
number, so the preview and the page selector show page 5 of the file even when
only pages 5–7 were read. Progress counts the selected pages.

While a PDF run is in flight the button becomes **Cancel**. The flag is checked
between pages, so the click answers at once and the run stops before starting
the next page; a page already inside ONNX Runtime finishes first, because the
runtime has no interruption point. Cancelling discards the pages of that run
instead of adopting a partial document — the previously loaded pages stay as
they were — and reports a neutral notice rather than an error. A single image
run has no between-page checkpoint, so it shows a spinner instead of a button
that could not answer. The shell keeps one cancellation token at a time and
compares it by pointer, so a run that finishes late cannot clear the token of
the run that replaced it.

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
| `.xlsx` | Real spreadsheet; one worksheet per page, named with the document's page number, RTL sheet view, merged cells, numeric cells kept as numbers, tables stacked with a row cursor instead of overwriting each other |

Export writes **all pages** currently loaded (`export_docx_pages` /
`export_xlsx_pages`), not only the active one — which, after a page window, is
the window rather than the whole document. The `PageResult`s travel to Rust
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
- **The PP-OCRv5 Arabic candidate was measured and is not the default yet.**
  `arabic_PP-OCRv5_mobile_rec_onnx` (8 MB, Apache-2.0) was compared on the same
  renders in two typefaces, clean and degraded (blur, sensor noise, JPEG). It
  is decisively better on clean letterforms once its output is converted from
  visual to logical order — character error rate 0.30 against 0.50 — but it
  still loses Arabic-Indic digit runs and mixed Arabic/Latin text, so with
  degraded and mixed lines the gap narrows to 0.60 against 0.65. Swapping the
  default on synthetic evidence alone would trade a known limitation for an
  unmeasured one; a labelled corpus of real Arabic scans is the missing input.
  `arabic_smoke.rs` pins the current exact-match baseline so the regression is
  caught either way.
- Very small text (below ~20 px) degrades both scripts, since recognition
  normalizes every line to 48 px tall.
- Layout is only as good as PicoDet's five classes. When the layout model does
  not call a grid a `table`, the pipeline still scans the region for ruling
  lines: three or more rows and columns of dark lines become a real grid, and a
  cell with no recognized text leaves the region as plain text. A table drawn
  without ruling lines (whitespace-separated columns) is not recovered.
