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
window. Both accept `png`, `jpg`, `jpeg`, `tif`, `tiff`, `bmp`, `webp`, `pdf`,
`docx`, `xlsx`, `odt`, `txt`, `md`, `csv`, `tsv` and `log`, both go through the same
adoption path, and a dropped file with any other extension is refused by name
instead of failing later inside the engine. Only
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

### Documents that already hold text

A `.docx`, `.xlsx`, `.odt` or text file is not a picture of words, so running the
detector over it would be the wrong tool even if it worked. The button says
**Extract text** for these formats, and the reader in `core/ocr_engine/src/
document.rs` opens the file directly:

- **`.docx`** — `word/document.xml`, paragraph by paragraph. Tabs and line
  breaks survive, entities are decoded, and pages split where Word recorded a
  break (`<w:lastRenderedPageBreak/>` or `<w:br w:type="page"/>`). A document
  Word never laid out becomes one page: the text is complete, the page count is
  simply unknown.
- **`.xlsx`** — every sheet, one page each (the sheet name heads the page when
  there is more than one). Shared strings, inline strings and booleans resolve;
  a cell is placed by its reference, so a sparse row keeps its columns aligned.
  A date, a percentage and a time are rendered the way Excel shows them
  (`2023-03-15`, `50.0%`, `12:00:00`), from the cell's number format: the style
  index resolves through `cellXfs` to a built-in id or a custom format code, and
  the workbook's 1900/1904 date system is honoured. A cell with no style keeps
  its stored digits, so `0.5` with no percent format still reads `0.5`.
  Formulas show their cached result, because that is the value the file carries;
  the formula text is not searched.
- **`.odt`** — `content.xml`, paragraph by paragraph (`text:p`, `text:h`). Tabs,
  explicit runs of spaces (`text:s`), line breaks and `text:soft-page-break`
  survive; inline styles and notes are skipped, and text inside frames is read
  in document order. Like a `.docx` without page breaks, a document with no soft
  breaks becomes one page.
- **`.txt`, `.md`, `.csv`, `.tsv`, `.log`** — UTF-8 (with or without a BOM) and
  UTF-16 (with a BOM, or the zero-byte pattern of BOM-less Latin text). A file
  that is not valid UTF-8 is tried as Windows-1256 and Windows-1252, which is
  how a legacy Arabic or Western file is actually saved on the machines that
  produce them. The decision is a documented, two-candidate heuristic rather
  than a statistical detector: at least 10% of the non-space characters must
  land in the Arabic block under 1256, otherwise 1252 is used. A file under 16
  bytes, or one where fewer than 75% of characters are text rather than control
  bytes, is refused with a message naming the encodings instead of producing
  mojibake. A form feed splits pages, `\r\n` becomes `\n`, and nothing else is
  rewritten.

The result is the same `PageResult` the pipeline returns, so the text pane, the
region editor, the `.docx`/`.xlsx` export and the content index all work
unchanged. The preview pane says a text document has no page image instead of
asking the PDF renderer for one. Extraction is bounded: 64 MB per file, 32 MB
per package entry, 2,000,000 characters and 500 pages per document, so a
malformed package cannot exhaust memory. Nothing is written to a temporary file
and nothing is sent anywhere.

## Photo input

A phone photo is not a flatbed scan, so the pipeline was measured against
degraded versions of the known twelve-cell table fixture
(`core/ocr_engine/tests/photo_quality.rs`, run by `make ocr-test`): low
contrast, a lighting gradient, 1.6 px blur, deterministic sensor noise, JPEG at
quality 45, and all of them combined. The combined case still reads 10 of 12
cells and every single degradation reads all 12 except sensor noise at 11, so
no preprocessing step is enabled. Contrast stretching, sharpening or denoising
would each change pixels the models already read correctly, and there is no
evidence yet that a transform recovers the two lost cells instead of costing
others. The test keeps the measured floor as a regression gate: any future
enhancement has to beat it on this same fixture, and the numbers are printed on
every run so a model update is visible rather than assumed.

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

## Searching by content

A finished run stores its text in the local index, so the launcher finds the
file by a phrase inside it: type `revenue` and the scanned report appears with
an excerpt. Nothing is uploaded — the body comes from the engine that just ran
on this machine and goes into the same SQLite file as the file index.

- **Where it lives.** `document_text` holds one row per path (re-running OCR
  replaces it); `document_token` is an inverted index of the same normalized
  tokens search uses, so a query costs one lookup per token instead of a scan.
  Both tables are created by the store's migration.
- **What ranks where.** Name matches come first, then up to two content hits,
  then the web row. A file already matched by name is not repeated.
- **The switch.** Settings → Indexing → *Index recognized text* is on by
  default. Turning it off stops new writes; it does not delete what is stored.
  *Clear indexed text* does, and leaves the files alone.
- **Limits.** A document contributes at most 400,000 characters, and the index
  keeps at most 20,000 documents (oldest dropped first). Short queries — under
  three characters — skip content search, because one letter matches nearly
  every page.
- **Edits are respected.** A run stores the first pass; exporting stores the
  pages as you finalized them, so the index describes what you saved.

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

The bundled set is the **Lightweight** tier. The **OCR Models** screen adds two
larger packs — a server-grade detector and a PP-OCRv5 set with the v5 Arabic
recognizer — which replace detection and recognition while layout and table
stay shared. Installs are downloaded, SHA-256 checked, staged and verified
before activation. See [OCR models and the advisor](ocr-models.md).

The ONNX Runtime and pdfium shared libraries are loaded dynamically. Drop them
in `src-tauri/resources/lib/` before packaging (the bundler copies them to
`<resources>/lib`), or point `ORT_DYLIB_PATH` / `ZEIDEX_PDFIUM_LIBRARY` at them.
`cuda` / `directml` cargo features are opt-in; the default is CPU.

## Limits

- No legacy `.doc` or `.ppt`: those are binary OLE formats whose text lives in
  compressed streams, not XML. The message names the format instead of guessing.
  `.odt` is read (§ "Documents that already hold text").
- A text file that is neither UTF-8, UTF-16, Windows-1256 nor Windows-1252 is
  refused, not mangled. The legacy code pages are tried with an explicit,
  documented rule (10% Arabic under 1256 wins, otherwise 1252); a file that
  cannot plausibly be either is refused rather than decoded into characters that
  look right in the preview and wrong in the index.
- Spreadsheet formulas are not evaluated: a formula cell shows the cached result
  the file stores. Dates, percentages and times are rendered from the cell's
  number format, so a date reads `2023-03-15` rather than `45000`.

- **Arabic accuracy is model-bound.** `arabic_PP-OCRv4_rec` reads shaped Arabic
  in logical order correctly, but loses characters as a line gets denser. On
  synthetic Noto Kufi renders at 40 px it is exact for
  `مرحبا بكم في لوميو` and `كتاب الطالب` (0.82–0.88 confidence) and drops
  Arabic-Indic digits (`١٢٣٤٥` → `٢٣٥`). This is a model limit, not a pipeline
  bug; replacing `ppocrv4_rec_ar.onnx` and `arabic_dict.txt` is the upgrade
  path, and the editor exists because of it.
- **The PP-OCRv5 Arabic recognizer is opt-in, not the default.**
  `arabic_PP-OCRv5_mobile_rec_onnx` (8 MB, Apache-2.0) was compared on the same
  renders in two typefaces, clean and degraded (blur, sensor noise, JPEG). It
  is decisively better on clean letterforms once its output is converted from
  visual to logical order — character error rate 0.30 against 0.50 — but it
  still loses Arabic-Indic digit runs and mixed Arabic/Latin text, so with
  degraded and mixed lines the gap narrows to 0.60 against 0.65. Swapping the
  default on synthetic evidence alone would trade a known limitation for an
  unmeasured one; a labelled corpus of real Arabic scans is the missing input.
  It ships as the Arabic recognizer of the **Advanced** pack, so a user can
  choose it with full knowledge of the trade-off; the default stays v4.
  `arabic_smoke.rs` pins the current exact-match baseline so the regression is
  caught either way.
- Very small text (below ~20 px) degrades both scripts, since recognition
  normalizes every line to 48 px tall.
- Layout is only as good as PicoDet's five classes. When the layout model does
  not call a grid a `table`, the pipeline still scans the region for ruling
  lines: three or more rows and columns of dark lines become a real grid, and a
  cell with no recognized text leaves the region as plain text. A table drawn
  without ruling lines (whitespace-separated columns) is not recovered.
